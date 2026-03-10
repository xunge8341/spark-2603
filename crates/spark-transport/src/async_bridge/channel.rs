use std::sync::Arc;
use std::time::Duration;

use spark_buffer::Bytes;

use crate::evidence::EvidenceSink;
use crate::io::MsgBoundary;
use crate::policy::{FlushBudget, FlushPolicy};
use crate::reactor::Interest;
use crate::{KernelError, Result};
use spark_core::service::Service;

use super::channel_state::{ChannelState, ReadChunk};
use super::dyn_channel::DynChannel;
use super::outbound_buffer::{FlushStatus, WritabilityChange};
use super::pipeline::FrameDecoderProfile;
use super::pipeline::{
    ChannelPipeline, ChannelPipelineBuilder, DelimiterOptions, Http1Options, LengthFieldOptions,
    LineOptions, Varint32Options,
};
use super::task::AppFuture;
use super::OutboundFrame;

pub type OnReadableStats = (usize, usize, usize, u64, u64, u64, u64, u64, u64, u64);

/// Netty/DotNetty 风格的“语义 Channel”（每连接一个）。
///
/// 组成：
/// - `ChannelState`：IO + outbound buffer + backpressure。
/// - `ChannelPipeline`：inbound/outbound handler 链。
///
/// 说明：
/// - IO backend 被隐藏在 `DynChannel` 中；上层只通过 Channel 语义对象交互。
/// - bring-up 阶段 pipeline 固定链；后续可扩展为可插拔 handler。
pub struct Channel<A, Ev = Arc<dyn EvidenceSink>, Io = Box<dyn DynChannel>>
where
    A: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
    Ev: EvidenceSink,
    Io: DynChannel,
{
    state: ChannelState<Ev, Io>,
    pipeline: ChannelPipeline<A, Ev, Io>,
}

/// Numeric limits for a [`Channel`].
///
/// This groups closely related knobs to keep constructor signatures short and
/// developer-friendly under `-D warnings`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelLimits {
    pub max_frame: usize,
    pub high_watermark: usize,
    pub low_watermark: usize,
    pub max_pending_write_bytes: usize,
}

impl ChannelLimits {
    #[inline]
    pub fn new(
        max_frame: usize,
        high_watermark: usize,
        low_watermark: usize,
        max_pending_write_bytes: usize,
    ) -> Self {
        Self {
            max_frame: max_frame.max(1),
            high_watermark,
            low_watermark,
            max_pending_write_bytes,
        }
    }
}

impl<A, Ev, Io> Channel<A, Ev, Io>
where
    A: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
    Ev: EvidenceSink,
    Io: DynChannel,
{
    // 说明：本类型中的部分方法属于“对外 Channel 语义 API”。
    // 当前 dataplane 主链路由 driver 事件驱动，不一定在本 crate 内部直接调用到这些方法。
    // 为避免 `dead_code` 噪声，同时保留对上层集成/测试的可用性，在相关方法上做定向 allow。

    pub fn new(
        chan_id: u32,
        io: Io,
        max_frame: usize,
        high_watermark: usize,
        low_watermark: usize,
        app: Arc<A>,
        evidence: Ev,
    ) -> Self {
        let limits = ChannelLimits::new(max_frame, high_watermark, low_watermark, usize::MAX);
        let flush_budget = FlushPolicy::default().budget(limits.max_frame);
        Self::new_with_profile_and_flush_budget(
            chan_id,
            io,
            FrameDecoderProfile::line(limits.max_frame),
            limits,
            flush_budget,
            app,
            evidence,
        )
    }

    pub fn new_with_flush_budget(
        chan_id: u32,
        io: Io,
        limits: ChannelLimits,
        flush_budget: FlushBudget,
        app: Arc<A>,
        evidence: Ev,
    ) -> Self {
        Self::new_with_profile_and_flush_budget(
            chan_id,
            io,
            FrameDecoderProfile::line(limits.max_frame),
            limits,
            flush_budget,
            app,
            evidence,
        )
    }

    pub fn new_with_profile_and_flush_budget(
        chan_id: u32,
        io: Io,
        profile: FrameDecoderProfile,
        limits: ChannelLimits,
        flush_budget: FlushBudget,
        app: Arc<A>,
        evidence: Ev,
    ) -> Self {
        let mut state = ChannelState::new(
            chan_id,
            io,
            limits.high_watermark,
            limits.low_watermark,
            limits.max_pending_write_bytes,
            flush_budget,
            evidence,
        );

        let mut b = ChannelPipelineBuilder::<A, Ev, Io>::new(app);
        b = match profile {
            FrameDecoderProfile::Line { max_frame } => b.line(LineOptions::new(max_frame)),
            FrameDecoderProfile::Delimiter {
                max_frame,
                delimiter,
                include_delimiter,
            } => b.delimiter(DelimiterOptions::new(
                max_frame,
                delimiter,
                include_delimiter,
            )),
            FrameDecoderProfile::LengthField {
                max_frame,
                field_len,
                order,
            } => {
                if let Some(opts) = LengthFieldOptions::new(max_frame, field_len as usize, order) {
                    b.length_field(opts)
                } else {
                    b.framing(profile)
                }
            }
            FrameDecoderProfile::Varint32 { max_frame } => {
                b.varint32(Varint32Options::new(max_frame))
            }
            FrameDecoderProfile::Http1 {
                max_request_bytes,
                max_head_bytes,
                max_headers,
            } => b.http1(Http1Options::with_limits(
                max_request_bytes,
                max_head_bytes,
                max_headers,
            )),
        };
        let mut pipeline = b.build();

        let _ = pipeline.fire_channel_active(&mut state);
        Self { state, pipeline }
    }

    #[inline]
    #[allow(dead_code)]
    pub fn chan_id(&self) -> u32 {
        self.state.chan_id()
    }

    #[inline]
    pub fn io_mut(&mut self) -> &mut Io {
        self.state.io_mut()
    }

    #[inline]
    pub fn is_read_paused(&self) -> bool {
        self.state.is_read_paused()
    }

    #[inline]
    pub fn peer_eof(&self) -> bool {
        self.state.peer_eof()
    }

    #[inline]
    pub fn desired_interest(&self) -> Interest {
        self.state.desired_interest()
    }

    // ---------------- outbound public API（Netty/DotNetty 语义） ----------------

    /// 写入一段 bytes（语义：write/enqueue）。
    ///
    /// 说明：
    /// - 对齐 Netty/DotNetty：`channel.write()` 从 **tail** 开始（outbound 方向 tail->head）。
    /// - 与 `ctx.write()` 的区别：ctx.write 从“当前 handler”向 head 传播。
    #[allow(dead_code)]
    pub fn write(&mut self, msg: Bytes) -> Result<()> {
        self.pipeline
            .write(&mut self.state, OutboundFrame::from_bytes(msg))
    }

    /// flush 事件（语义：让 encoder/聚合器把内部缓冲写入 outbound）。
    ///
    /// 注意：真实 I/O drain 仍由 driver 在合适时机统一调用 `flush_outbound()` 完成。
    #[allow(dead_code)]
    pub fn flush(&mut self) -> Result<()> {
        self.pipeline.flush(&mut self.state)
    }

    /// 关闭连接（语义：close）。
    ///
    /// 说明：
    /// - outbound 方向从 tail 开始传播；最终由 HeadHandler 落实到 `state.request_close()`。
    #[allow(dead_code)]
    pub fn close(&mut self) -> Result<()> {
        self.pipeline.close(&mut self.state)
    }

    /// 处理 readable 事件：read loop + pipeline（decoder）处理。
    ///
    /// 返回：(read_bytes, decoded_msgs, decode_errors, too_large, coalesce_count, copied_bytes, cumulation_copy_bytes, lease_tokens, lease_borrowed_bytes, materialize_bytes)
    pub fn on_readable(
        &mut self,
        read_buf: &mut [u8],
        max_reads_per_call: usize,
    ) -> Result<OnReadableStats> {
        let mut read_bytes = 0usize;

        let mut reads = 0usize;
        while reads < max_reads_per_call {
            reads += 1;
            let (state, pipeline) = (&mut self.state, &mut self.pipeline);
            let (boundary, had_data) =
                match state.with_read_chunk(read_buf, |state, boundary, chunk| {
                    if chunk.is_empty() {
                        return Ok((boundary, false));
                    }
                    read_bytes += chunk.len();

                    match chunk {
                        ReadChunk::Borrowed(b) => {
                            debug_assert_eq!(boundary, MsgBoundary::None);
                            // Fast-path for stream reads: avoid intermediate Bytes allocation/copy.
                            pipeline.fire_channel_read_raw_stream_slice(state, b)?;
                        }
                        ReadChunk::Owned(bytes) => {
                            // Owned bytes path: materialized token fallback, or datagram payload.
                            pipeline.fire_channel_read_raw(state, boundary, bytes)?;
                        }
                    }

                    Ok((boundary, true))
                }) {
                    Ok(v) => v,
                    Err(KernelError::WouldBlock) => break,
                    Err(KernelError::Interrupted) => continue,
                    Err(KernelError::Eof) => {
                        // Peer half-close: stop READ interest but allow pending writes to flush.
                        self.state.mark_peer_eof();
                        break;
                    }
                    Err(KernelError::Closed) => return Err(KernelError::Closed),
                    Err(e) => return Err(e),
                };

            if !had_data {
                break;
            }

            // Datagram 一般一次就够；但 stream 允许 read spin。
            if boundary == MsgBoundary::Complete {
                break;
            }
        }

        self.pipeline.fire_channel_read_complete(&mut self.state)?;

        let (
            decoded,
            decode_errs,
            too_large,
            coalesce_count,
            copied_bytes,
            cumulation_copy_bytes,
            lease_tokens,
            lease_borrowed_bytes,
            materialize_bytes,
        ) = self.state.take_decode_stats();
        Ok((
            read_bytes,
            decoded,
            decode_errs,
            too_large,
            coalesce_count,
            copied_bytes,
            cumulation_copy_bytes,
            lease_tokens,
            lease_borrowed_bytes,
            materialize_bytes,
        ))
    }

    /// 尝试 flush outbound buffer。
    ///
    /// 返回：(status, bytes_written, syscalls, writev_calls)
    pub fn flush_outbound(&mut self) -> (FlushStatus, usize, u64, u64) {
        self.state.flush_outbound()
    }

    /// 取走本轮 pending 的可写性变化。
    #[inline]
    pub fn take_writability_change(&mut self) -> WritabilityChange {
        self.state.take_writability_change()
    }

    #[inline]
    pub fn is_writable(&self) -> bool {
        self.state.is_writable()
    }

    #[inline]
    pub fn is_draining(&self) -> bool {
        self.state.is_draining()
    }

    #[inline]
    pub fn is_close_requested(&self) -> bool {
        self.state.is_close_requested()
    }

    /// 进入 draining（Channel 级）：停止读 + 允许写 flush + 关闭策略。
    #[allow(dead_code)]
    pub fn enter_draining(&mut self, close_after_flush: bool, timeout: Duration, inflight: u32) {
        self.state
            .enter_draining(close_after_flush, timeout, inflight);
    }

    /// driver：推进 draining 状态机。
    pub fn poll_draining(&mut self, inflight: u32) {
        self.state.poll_draining(inflight);
    }

    /// driver：从 pipeline 取出一个待 poll 的业务 future。
    pub fn take_app_future(&mut self) -> Option<AppFuture> {
        self.pipeline.take_app_future()
    }

    /// driver：是否有 backlog 请求。
    pub fn has_app_backlog(&self) -> bool {
        self.pipeline.has_app_backlog()
    }

    /// driver：业务 future 完成后回调。
    pub fn on_app_complete(&mut self, result: core::result::Result<Option<Bytes>, KernelError>) {
        let _ = self.pipeline.fire_app_complete(&mut self.state, result);
    }

    /// driver：连接失活（reactor Closed）。
    ///
    /// 说明：
    /// - 后端/driver 收到 Closed 事件时调用，让 pipeline 有机会清理内部状态（cumulation/backlog 等）。
    /// - 该回调不返回错误；pipeline 内部错误会以 exceptionCaught 事件向后传播（best-effort）。
    pub fn on_inactive(&mut self) {
        let _ = self.pipeline.fire_channel_inactive(&mut self.state);
    }

    /// Driver-only helper: force close on I/O error and record a stable close cause.
    ///
    /// Contract:
    /// - Must be panic-free and allocation-free in hot paths.
    /// - Ensures CloseRequested evidence is emitted exactly once.
    pub(crate) fn force_close_on_error(&mut self, err: KernelError) {
        // Emit CloseRequested (idempotent).
        self.state.request_close();
        // Record the actual cause for CloseComplete / AbortiveClose.
        self.state.note_close_error(err);
    }

    /// driver：触发 writabilityChanged。
    pub fn fire_writability_changed(&mut self, is_writable: bool) {
        let _ = self
            .pipeline
            .fire_channel_writability_changed(&mut self.state, is_writable);
    }
}
