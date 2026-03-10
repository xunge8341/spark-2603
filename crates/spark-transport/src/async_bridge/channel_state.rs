use crate::io::{MsgBoundary, ReadData};
use crate::policy::FlushBudget;
use crate::reactor::Interest;
use crate::uci::names::evidence as ev_names;
use crate::uci::EvidenceEvent;
use crate::{KernelError, Result, RxToken};

use spark_buffer::Bytes;
use std::time::{Duration, Instant};

use super::dyn_channel::DynChannel;
use super::outbound_buffer::{FlushStatus, OutboundBuffer, WritabilityChange};
use super::outbound_frame::OutboundFrame;
use crate::evidence::EvidenceSink;

/// 从 `RxToken` 解码出 `chan_id`（编码格式：`(chan_id<<32) | gen`）。
#[inline]
pub fn tok_chan_id(tok: RxToken) -> u32 {
    (tok.0 >> 32) as u32
}

/// A single read chunk returned by `ChannelState::read_once`.
///
/// Motivation:
/// - Stream (TCP) reads are typically immediately copied into the cumulation buffer.
///   Returning a borrowed slice avoids an extra intermediate allocation/copy.
/// - Datagram reads must return an owned `Bytes` because the payload may be forwarded
///   to the application layer.
#[derive(Debug)]
pub enum ReadChunk<'a> {
    /// Borrowed bytes only valid within the current pipeline call stack.
    ///
    /// Contract:
    /// - Never store this slice across handler/state/queue boundaries.
    /// - Never move it into async tasks/futures.
    Borrowed(&'a [u8]),
    /// Owned bytes (e.g. leased token materialized, or datagram copy).
    Owned(Bytes),
}

impl<'a> ReadChunk<'a> {
    #[inline]
    pub fn len(&self) -> usize {
        match self {
            ReadChunk::Borrowed(b) => b.len(),
            ReadChunk::Owned(b) => b.len(),
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Close cause used for observability and contract tests.
///
/// Contract:
/// - This is best-effort (no allocation, no strings).
/// - It must remain stable across backends/OS to support production diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseCause {
    None,
    /// Local requested close (explicit close API or internal shutdown).
    Requested,
    /// Peer half-closed its write side (FIN observed as EOF).
    PeerHalfClose,
    /// I/O error forced the channel to close (e.g. Reset/Timeout).
    Error(KernelError),
}

/// 每连接的“语义状态机”承载体（Netty/DotNetty 风格的 Channel state）。
///
/// 说明：
/// - 这里 **不** 承载 pipeline/decoder（由 `Channel` 组合）。
/// - 这里承载 IO 句柄 + outbound buffer + backpressure 状态。
/// - 未来接 io_uring 时，差异应尽量被限制在 `IoOps` 这一层。
///
/// Dev-friendly design:
/// - 默认 IO 类型为 `Box<dyn DynChannel>`（运行时中立、便于 bring-up）。
/// - 后端实现（mio/uring/serial）可选择用具体 `Io` 类型（静态分派、热路径无 vtable）。
pub struct ChannelState<Ev, Io = Box<dyn DynChannel>>
where
    Ev: EvidenceSink,
    Io: DynChannel,
{
    chan_id: u32,
    io: Io,
    outbound: OutboundBuffer,
    flush_budget: FlushBudget,

    evidence: Ev,

    // 当进入 backpressure（不可写）时，暂停读。
    read_paused: bool,

    // 是否已请求关闭（pipeline close 语义触发）。
    // 说明：
    // - close 请求会尽快调用底层 io.close()；
    // - reactor 在后续 poll 中通常会产生 Closed 事件，driver 将移除该 channel。
    close_requested: bool,

    // Best-effort close cause for evidence/diagnostics.
    close_cause: CloseCause,
    // Ensure CloseComplete evidence is emitted at most once.
    close_complete_emitted: bool,

    // Draining 状态（Channel 级）：停止读、允许写 flush，并在满足条件后 close。
    draining: bool,
    draining_deadline: Option<Instant>,
    close_after_flush: bool,

    // Peer half-close observed (read-side EOF).
    //
    // Contract:
    // - EOF must suppress further READ interest to avoid reactor busy loops.
    // - EOF must NOT imply write is closed; the channel may still flush pending outbound.
    peer_eof: bool,

    // 写路径可写性变化（本轮 event-loop 取走并触发 pipeline 事件）。
    pending_writability: WritabilityChange,

    // 本次 on_readable 的 decode 统计（由 decoder handler 累加）。
    decoded_acc: usize,
    decode_err_acc: usize,
    // 本次 on_readable 的 cumulation 统计（copy/coalesce）。
    inbound_coalesce_acc: u64,
    inbound_copied_bytes_acc: u64,
    inbound_frame_too_large_acc: u64,
    rx_cumulation_copy_bytes_acc: u64,
    rx_lease_tokens_acc: u64,
    rx_lease_borrowed_bytes_acc: u64,
    rx_materialize_bytes_acc: u64,
}

impl<Ev, Io> core::fmt::Debug for ChannelState<Ev, Io>
where
    Ev: EvidenceSink,
    Io: DynChannel,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // 注意：`DynChannel` 可能不实现 Debug（例如内部包裹 socket/mio handle）。
        // 这里仅输出对排障有价值的状态字段。
        f.debug_struct("ChannelState")
            .field("chan_id", &self.chan_id)
            .field("read_paused", &self.read_paused)
            .field("pending_writability", &self.pending_writability)
            .field("decoded_acc", &self.decoded_acc)
            .field("decode_err_acc", &self.decode_err_acc)
            .field("peer_eof", &self.peer_eof)
            .field("inbound_coalesce_acc", &self.inbound_coalesce_acc)
            .field("inbound_copied_bytes_acc", &self.inbound_copied_bytes_acc)
            .field(
                "inbound_frame_too_large_acc",
                &self.inbound_frame_too_large_acc,
            )
            .finish()
    }
}

impl<Ev, Io> ChannelState<Ev, Io>
where
    Ev: EvidenceSink,
    Io: DynChannel,
{
    pub fn new(
        chan_id: u32,
        io: Io,
        high_watermark: usize,
        low_watermark: usize,
        flush_budget: FlushBudget,
        evidence: Ev,
    ) -> Self {
        Self {
            chan_id,
            io,
            outbound: OutboundBuffer::new(high_watermark, low_watermark),
            flush_budget,
            evidence,
            read_paused: false,
            close_requested: false,
            close_cause: CloseCause::None,
            close_complete_emitted: false,

            draining: false,
            draining_deadline: None,
            close_after_flush: false,
            peer_eof: false,
            pending_writability: WritabilityChange::None,
            decoded_acc: 0,
            decode_err_acc: 0,
            inbound_coalesce_acc: 0,
            inbound_copied_bytes_acc: 0,
            inbound_frame_too_large_acc: 0,
            rx_cumulation_copy_bytes_acc: 0,
            rx_lease_tokens_acc: 0,
            rx_lease_borrowed_bytes_acc: 0,
            rx_materialize_bytes_acc: 0,
        }
    }

    #[inline]
    #[allow(dead_code)]
    pub fn chan_id(&self) -> u32 {
        self.chan_id
    }

    #[inline]
    pub fn io_mut(&mut self) -> &mut Io {
        &mut self.io
    }

    #[inline]
    pub fn is_read_paused(&self) -> bool {
        self.read_paused
    }

    /// Whether a peer half-close (read-side EOF) has been observed.
    #[inline]
    pub fn peer_eof(&self) -> bool {
        self.peer_eof
    }

    /// Mark a peer half-close (read-side EOF).
    ///
    /// This suppresses further READ interest to avoid reactor busy loops.
    /// It does not request close immediately; the driver may still flush pending outbound.
    #[inline]
    pub fn mark_peer_eof(&mut self) {
        if self.peer_eof {
            return;
        }
        self.peer_eof = true;
        self.read_paused = true;

        if matches!(self.close_cause, CloseCause::None) {
            self.close_cause = CloseCause::PeerHalfClose;
        }

        self.evidence.emit(EvidenceEvent {
            name: ev_names::PEER_HALF_CLOSE,
            reason: "peer_eof",
            value: 1,
            unit: "count",
            unit_mapping: ev_names::UNITMAP_CLOSE_V1,
            scope: "channel",
            scope_id: self.chan_id as u64,
            channel_id: self.chan_id,
            pending_write_bytes: self.outbound.bytes_total() as u64,
            inflight: 0,
        });
    }

    /// 根据当前状态计算 reactor interest。
    ///
    /// Netty/DotNetty 经验：
    /// - 默认总是监听 READ（除非 backpressure 暂停读）
    /// - 只有 outbound backlog 非空时才监听 WRITE
    #[inline]
    pub fn desired_interest(&self) -> Interest {
        let has_outbound = !self.outbound.is_empty();

        // 严格契约（T-01）：
        // - 写队列非空 => 需要 WRITE interest
        // - 暂停读（PauseRead） => 不监听 READ
        // - 若既暂停读且无写 backlog，则返回 empty（mio backend 将执行 deregister）
        // 说明：
        // - close_requested 会在 request_close() 内置位，并同时 pause read。
        // - 在 no-backlog 情况下返回 empty，可避免 reactor busy loop；driver 会把该连接
        //   视为“可回收”，并在同一 tick 释放资源（见 ChannelDriver::tick）。
        match (self.read_paused, has_outbound) {
            (true, true) => Interest::WRITE,
            (true, false) => Interest::empty(),
            (false, true) => Interest::READ.union(Interest::WRITE),
            (false, false) => Interest::READ,
        }
    }

    /// 请求关闭连接（Netty/DotNetty：close 语义）。
    ///
    /// 中文说明：
    /// - 该方法会调用底层 IO 的 `close()`（若后端支持）。
    /// - 即使后端返回 Unsupported，我们也把 close_requested 置位，避免继续读写。
    /// - driver 侧会在 reactor 报 Closed 时移除连接。
    pub fn request_close(&mut self) {
        if self.close_requested {
            return;
        }
        self.close_requested = true;
        self.read_paused = true;

        if matches!(self.close_cause, CloseCause::None) {
            self.close_cause = CloseCause::Requested;
        }

        self.evidence.emit(EvidenceEvent {
            name: ev_names::CLOSE_REQUESTED,
            reason: "request_close",
            value: 1,
            unit: "count",
            unit_mapping: ev_names::UNITMAP_CLOSE_V1,
            scope: "channel",
            scope_id: self.chan_id as u64,
            channel_id: self.chan_id,
            pending_write_bytes: self.outbound.bytes_total() as u64,
            inflight: 0,
        });

        let _ = self.io.close();
    }

    /// 进入 draining（Channel 级）。
    ///
    /// 动作：停止读 + 允许写 flush；满足条件后关闭。
    pub fn enter_draining(&mut self, close_after_flush: bool, timeout: Duration, inflight: u32) {
        if self.draining {
            return;
        }
        self.draining = true;
        self.read_paused = true;
        self.close_after_flush = close_after_flush;
        self.draining_deadline = Some(Instant::now() + timeout);

        self.evidence.emit(EvidenceEvent {
            name: ev_names::DRAINING_ENTER,
            reason: "enter_draining",
            value: 1,
            unit: "count",
            unit_mapping: ev_names::UNITMAP_DRAINING_V1,
            scope: "channel",
            scope_id: self.chan_id as u64,
            channel_id: self.chan_id,
            pending_write_bytes: self.outbound.bytes_total() as u64,
            inflight,
        });
    }

    #[inline]
    pub fn is_draining(&self) -> bool {
        self.draining
    }

    /// 每 tick 检查 draining：满足条件后 close 或超时关闭。
    pub fn poll_draining(&mut self, inflight: u32) {
        if !self.draining {
            return;
        }

        let pending = self.outbound.bytes_total();

        if self.close_after_flush && pending == 0 && inflight == 0 {
            self.draining = false;
            self.evidence.emit(EvidenceEvent {
                name: ev_names::DRAINING_EXIT,
                reason: "flush_completed",
                value: 1,
                unit: "count",
                unit_mapping: ev_names::UNITMAP_DRAINING_V1,
                scope: "channel",
                scope_id: self.chan_id as u64,
                channel_id: self.chan_id,
                pending_write_bytes: 0,
                inflight,
            });
            self.request_close();
            return;
        }

        if let Some(deadline) = self.draining_deadline {
            if Instant::now() >= deadline {
                self.draining = false;
                self.evidence.emit(EvidenceEvent {
                    name: ev_names::DRAINING_TIMEOUT,
                    reason: "timeout_close",
                    value: 1,
                    unit: "count",
                    unit_mapping: ev_names::UNITMAP_DRAINING_V1,
                    scope: "channel",
                    scope_id: self.chan_id as u64,
                    channel_id: self.chan_id,
                    pending_write_bytes: pending as u64,
                    inflight,
                });
                self.request_close();
            }
        }
    }

    #[inline]
    #[allow(dead_code)]
    pub fn is_close_requested(&self) -> bool {
        self.close_requested
    }

    /// Record a close-causing I/O error for observability.
    ///
    /// Contract: this is best-effort and must not allocate.
    pub fn note_close_error(&mut self, err: KernelError) {
        self.close_cause = CloseCause::Error(err);
        self.close_requested = true;
        self.read_paused = true;
    }

    /// Emit a stable CloseComplete evidence event (at most once).
    pub fn emit_close_complete(&mut self) {
        if self.close_complete_emitted {
            return;
        }
        self.close_complete_emitted = true;

        let (reason, abortive) = match self.close_cause {
            CloseCause::Requested => ("requested", false),
            CloseCause::PeerHalfClose => ("peer_half_close", false),
            CloseCause::Error(KernelError::Reset) => ("reset", true),
            CloseCause::Error(KernelError::Timeout) => ("timeout", false),
            CloseCause::Error(_) => ("error", false),
            CloseCause::None => ("closed", false),
        };

        self.evidence.emit(EvidenceEvent {
            name: ev_names::CLOSE_COMPLETE,
            reason,
            value: 1,
            unit: "count",
            unit_mapping: ev_names::UNITMAP_CLOSE_V1,
            scope: "channel",
            scope_id: self.chan_id as u64,
            channel_id: self.chan_id,
            pending_write_bytes: self.outbound.bytes_total() as u64,
            inflight: 0,
        });

        if abortive {
            self.evidence.emit(EvidenceEvent {
                name: ev_names::ABORTIVE_CLOSE,
                reason,
                value: 1,
                unit: "count",
                unit_mapping: ev_names::UNITMAP_CLOSE_V1,
                scope: "channel",
                scope_id: self.chan_id as u64,
                channel_id: self.chan_id,
                pending_write_bytes: self.outbound.bytes_total() as u64,
                inflight: 0,
            });
        }
    }
    /// 当前 Channel 是否可写（水位线驱动）。
    #[inline]
    pub fn is_writable(&self) -> bool {
        self.outbound.is_writable()
    }

    /// 入队 outbound bytes（对应 Netty/DotNetty 的 `write()`）。
    ///
    /// 注意：这里只做排队与水位线更新，不做真正写出。
    pub fn enqueue_outbound(&mut self, frame: OutboundFrame) {
        let wc = self.outbound.enqueue(frame);
        self.apply_writability_change(wc);
    }

    /// 尝试 drain outbound（对应 Netty/DotNetty 的 `flush()`）。
    ///
    /// 返回：(flush 状态, 写出字节数, syscalls, writev_calls)
    pub fn flush_outbound(&mut self) -> (FlushStatus, usize, u64, u64) {
        let (st, wrote, syscalls, writev_calls, wc) =
            self.outbound.flush_into(&mut self.io, self.flush_budget);
        self.apply_writability_change(wc);

        if st == FlushStatus::Limited {
            let pending = self.outbound.bytes_total();
            self.evidence.emit(EvidenceEvent {
                name: ev_names::FLUSH_LIMITED,
                reason: "flush_budget",
                value: pending as u64,
                unit: "bytes",
                unit_mapping: ev_names::UNITMAP_FLUSH_LIMITED_V1,
                scope: "channel",
                scope_id: self.chan_id as u64,
                channel_id: self.chan_id,
                pending_write_bytes: pending as u64,
                inflight: 0,
            });
        }

        (st, wrote, syscalls, writev_calls)
    }

    /// 取走本轮 pending 的可写性变化（驱动层将触发 pipeline 的 writabilityChanged 事件）。
    #[inline]
    pub fn take_writability_change(&mut self) -> WritabilityChange {
        let wc = self.pending_writability;
        self.pending_writability = WritabilityChange::None;
        wc
    }

    #[inline]
    fn apply_writability_change(&mut self, wc: WritabilityChange) {
        match wc {
            WritabilityChange::None => {}
            WritabilityChange::BecameUnwritable { pending_bytes } => {
                self.read_paused = true;
                self.pending_writability = wc;

                self.evidence.emit(EvidenceEvent {
                    name: ev_names::BACKPRESSURE_ENTER,
                    reason: "high_watermark",
                    value: pending_bytes as u64,
                    unit: "bytes",
                    unit_mapping: ev_names::UNITMAP_BACKPRESSURE_BYTES_V1,
                    scope: "channel",
                    scope_id: self.chan_id as u64,
                    channel_id: self.chan_id,
                    pending_write_bytes: pending_bytes as u64,
                    inflight: 0,
                });
            }
            WritabilityChange::BecameWritable { pending_bytes } => {
                self.read_paused = false;
                self.pending_writability = wc;

                self.evidence.emit(EvidenceEvent {
                    name: ev_names::BACKPRESSURE_EXIT,
                    reason: "low_watermark",
                    value: pending_bytes as u64,
                    unit: "bytes",
                    unit_mapping: ev_names::UNITMAP_BACKPRESSURE_BYTES_V1,
                    scope: "channel",
                    scope_id: self.chan_id as u64,
                    channel_id: self.chan_id,
                    pending_write_bytes: pending_bytes as u64,
                    inflight: 0,
                });
            }
        }
    }

    /// Read one payload chunk and invoke `f` while the chunk is still valid.
    ///
    /// Design notes:
    /// - Stream token reads may stay borrowed for the duration of `f`.
    /// - Borrowed bytes are stack-scoped to this call and must not be retained.
    /// - Datagram payloads stay owned because they can cross queue/handler boundaries.
    /// - Token release stays structural (RAII), and happens exactly once after `f` returns.
    pub fn with_read_chunk<R, F>(&mut self, read_buf: &mut [u8], f: F) -> Result<R>
    where
        F: FnOnce(&mut Self, MsgBoundary, ReadChunk<'_>) -> Result<R>,
    {
        // Prefer lease; fall back to copy path when unsupported.
        match self.io.try_read_lease() {
            Ok(outcome) => match outcome.data {
                ReadData::Token(rx) => {
                    if outcome.boundary == MsgBoundary::None {
                        self.with_stream_token(rx, |state, chunk| {
                            f(state, MsgBoundary::None, chunk)
                        })
                    } else {
                        let bytes = materialize_rx_token(&mut self.io, rx)?;
                        self.record_rx_materialize_bytes(bytes.len());
                        f(self, outcome.boundary, ReadChunk::Owned(bytes))
                    }
                }
                ReadData::Copied => {
                    // Contract: `try_read_lease()` must never return `Copied`.
                    // Backends that cannot lease must return `Err(Unsupported)` and implement `try_read_into()`.
                    Err(KernelError::Internal(
                        crate::error_codes::ERR_LEASE_CONTRACT_COPIED,
                    ))
                }
            },
            Err(KernelError::Unsupported) => {
                let o2 = self.io.try_read_into(read_buf)?;
                let chunk = if o2.n == 0 {
                    ReadChunk::Borrowed(&[])
                } else if o2.boundary == MsgBoundary::None {
                    // Stream: borrow the caller buffer to avoid an extra copy.
                    ReadChunk::Borrowed(&read_buf[..o2.n])
                } else {
                    // Datagram: must be owned because it may be forwarded to the application.
                    ReadChunk::Owned(Bytes::copy_from_slice(&read_buf[..o2.n]))
                };
                f(self, o2.boundary, chunk)
            }
            Err(e) => Err(e),
        }
    }

    fn with_stream_token<R, F>(&mut self, tok: RxToken, f: F) -> Result<R>
    where
        F: FnOnce(&mut Self, ReadChunk<'_>) -> Result<R>,
    {
        struct RxTokenGuard<Io: DynChannel> {
            chan: *mut Io,
            tok: RxToken,
        }

        impl<Io: DynChannel> Drop for RxTokenGuard<Io> {
            fn drop(&mut self) {
                // SAFETY:
                // - `chan` originates from `&mut self.io` in this function.
                // - The guard never escapes this stack frame.
                // - `drop` runs exactly once, after `f` returns, so release remains structural.
                unsafe { (&mut *self.chan).release_rx(self.tok) };
            }
        }

        let Some((ptr, len)) = self.io.rx_ptr_len(tok) else {
            return Err(KernelError::Internal(
                crate::error_codes::ERR_RX_PTR_LEN_NONE,
            ));
        };

        // SAFETY:
        // - `rx_ptr_len(tok)` guarantees ptr..ptr+len remains valid until `release_rx(tok)`.
        // - `guard` keeps the token alive for the full duration of `f`.
        // - We only expose the slice to the current call; it is never stored in ChannelState.
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
        let guard = RxTokenGuard {
            chan: &mut self.io as *mut Io,
            tok,
        };
        self.record_rx_lease_borrowed(bytes.len());
        let result = f(self, ReadChunk::Borrowed(bytes));
        drop(guard);
        result
    }

    #[inline]
    fn record_rx_lease_borrowed(&mut self, bytes: usize) {
        self.rx_lease_tokens_acc = self.rx_lease_tokens_acc.saturating_add(1);
        self.rx_lease_borrowed_bytes_acc = self
            .rx_lease_borrowed_bytes_acc
            .saturating_add(bytes as u64);
    }

    #[inline]
    fn record_rx_materialize_bytes(&mut self, bytes: usize) {
        self.rx_materialize_bytes_acc = self.rx_materialize_bytes_acc.saturating_add(bytes as u64);
    }

    /// decode handler 调用：累加“本轮”decoded 计数。
    #[inline]
    pub fn record_decoded(&mut self, n: usize) {
        self.decoded_acc = self.decoded_acc.saturating_add(n);
    }

    /// decode handler 调用：累加“本轮”decode error 计数。
    #[inline]
    pub fn record_decode_error(&mut self) {
        self.decode_err_acc = self.decode_err_acc.saturating_add(1);
    }

    /// decode handler 调用：记录一次 frame/request 过大（用于 mgmt HTTP/1 / delimiter framing）。
    ///
    /// `size` 是观测到的当前累积大小；`max` 是该 profile 的上限。
    #[inline]
    pub fn record_frame_too_large(&mut self, size: usize, max: usize) {
        self.inbound_frame_too_large_acc = self.inbound_frame_too_large_acc.saturating_add(1);

        self.evidence.emit(EvidenceEvent {
            name: ev_names::FRAME_TOO_LARGE,
            reason: "max_frame",
            value: size as u64,
            unit: "bytes",
            unit_mapping: ev_names::UNITMAP_FRAME_TOO_LARGE_V1,
            scope: "channel",
            scope_id: self.chan_id as u64,
            channel_id: self.chan_id,
            // Reuse pending_write_bytes as an auxiliary field to surface the configured limit.
            pending_write_bytes: max as u64,
            inflight: 0,
        });
    }

    #[inline]
    pub fn record_rx_cumulation_copy_bytes(&mut self, copied_bytes: usize) {
        if copied_bytes > 0 {
            self.rx_cumulation_copy_bytes_acc = self
                .rx_cumulation_copy_bytes_acc
                .saturating_add(copied_bytes as u64);
        }
    }

    /// decode handler 调用：记录本轮 cumulation copy/coalesce 统计。
    #[inline]
    pub fn record_inbound_cumulation(&mut self, coalesce_count: usize, copied_bytes: usize) {
        if coalesce_count > 0 {
            self.inbound_coalesce_acc = self
                .inbound_coalesce_acc
                .saturating_add(coalesce_count as u64);
        }
        if copied_bytes > 0 {
            self.inbound_copied_bytes_acc = self
                .inbound_copied_bytes_acc
                .saturating_add(copied_bytes as u64);
        }
    }

    /// 取走本轮 decode 统计。
    #[inline]
    pub fn take_decode_stats(&mut self) -> (usize, usize, u64, u64, u64, u64, u64, u64, u64) {
        let d = self.decoded_acc;
        let e = self.decode_err_acc;
        let c = self.inbound_coalesce_acc;
        let b = self.inbound_copied_bytes_acc;
        let tl = self.inbound_frame_too_large_acc;
        let cumulation_copy_bytes = self.rx_cumulation_copy_bytes_acc;
        let lease_tokens = self.rx_lease_tokens_acc;
        let lease_borrowed = self.rx_lease_borrowed_bytes_acc;
        let materialize_bytes = self.rx_materialize_bytes_acc;
        self.decoded_acc = 0;
        self.decode_err_acc = 0;
        self.inbound_coalesce_acc = 0;
        self.inbound_copied_bytes_acc = 0;
        self.inbound_frame_too_large_acc = 0;
        self.rx_cumulation_copy_bytes_acc = 0;
        self.rx_lease_tokens_acc = 0;
        self.rx_lease_borrowed_bytes_acc = 0;
        self.rx_materialize_bytes_acc = 0;
        (
            d,
            e,
            tl,
            c,
            b,
            cumulation_copy_bytes,
            lease_tokens,
            lease_borrowed,
            materialize_bytes,
        )
    }
}

/// 将 zero-copy RX token materialize 为 owned bytes。
///
/// 说明：
/// - 这是 trunk 当前刻意保留的“安全优先”形态：先 materialize，再进入 decode。
/// - 在 Driver Scheduling Kernel（ADR-013）和 future completion submit 的所有权边界
///   还未完全冻结前，不要把这里直接扩展成跨调用借用视图；否则容易把生命周期复杂度
///   提前扩散到主干。
/// - 后续可在有内部指标支撑时，迭代到 `Bytes`/池化/arena/零拷贝 view。
fn materialize_rx_token(chan: &mut dyn DynChannel, tok: RxToken) -> Result<Bytes> {
    // Ensure token release is structural (RAII), even on early-return paths.
    struct RxTokenGuard<'a> {
        chan: &'a mut dyn DynChannel,
        tok: RxToken,
    }

    impl<'a> Drop for RxTokenGuard<'a> {
        fn drop(&mut self) {
            self.chan.release_rx(self.tok);
        }
    }

    let g = RxTokenGuard { chan, tok };

    // bring-up 阶段：统一通过 IoOps 的 token-view（可选）接口 materialize。
    // 这样 transport 语义层不会绑定任何具体后端类型（TCP/UDP/串口/…）。
    let Some((ptr, len)) = g.chan.rx_ptr_len(tok) else {
        return Err(KernelError::Internal(
            crate::error_codes::ERR_RX_PTR_LEN_NONE,
        ));
    };

    // materialize: copy raw parts into owned bytes.
    let bytes = crate::lease::copy_from_parts(ptr, len);
    drop(g);
    Ok(Bytes::from(bytes))
}
