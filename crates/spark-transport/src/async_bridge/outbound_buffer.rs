use std::collections::VecDeque;

use super::outbound_frame::OutboundFrame;

use crate::io::IoOps;
use crate::policy::FlushBudget;
use crate::KernelError;

/// Netty-style outbound buffer with high/low watermarks.
///
/// Semantics:
/// - `write()` enqueues frames.
/// - `flush()` drains as much as possible to the underlying non-blocking IO.
/// - Writability flips to `false` when `bytes_total >= high`.
/// - Writability flips back to `true` when `bytes_total <= low`.
///
/// Design goals:
/// - Support allocation-free vectored writes (writev/WSASend) by queueing small segmented frames.
/// - Preserve partial-write progress across segments without copying.
#[derive(Debug)]
pub struct OutboundBuffer {
    q: VecDeque<OutboundFrame>,
    head_seg: usize,
    head_off: usize,
    bytes_total: usize,

    high: usize,
    low: usize,
    writable: bool,
}

#[derive(Debug, Clone, Copy)]
struct IovMeta {
    /// Frame index relative to the current front of the queue.
    frame_off: usize,
    /// Segment index within the frame.
    seg: usize,
    /// Byte offset within the segment.
    off: usize,
    /// Length of the corresponding iov slice.
    len: usize,
}

impl IovMeta {
    const EMPTY: Self = Self {
        frame_off: 0,
        seg: 0,
        off: 0,
        len: 0,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlushStatus {
    /// Buffer fully drained.
    Drained,
    /// WouldBlock; needs WRITE interest.
    WouldBlock,
    /// Flush budget reached; needs a follow-up flush in a later tick.
    Limited,
    /// Channel closed.
    Closed,
    /// Other error.
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WritabilityChange {
    None,
    BecameWritable { pending_bytes: usize },
    BecameUnwritable { pending_bytes: usize },
}

impl OutboundBuffer {
    pub fn new(high: usize, low: usize) -> Self {
        debug_assert!(high >= low);
        Self {
            q: VecDeque::new(),
            head_seg: 0,
            head_off: 0,
            bytes_total: 0,
            high: high.max(1),
            low: low.min(high).max(0),
            writable: true,
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.q.is_empty()
    }

    #[inline]
    /// 当前 outbound 队列总字节数（用于调试/观测）。
    pub fn bytes_total(&self) -> usize {
        self.bytes_total
    }

    #[inline]
    pub fn is_writable(&self) -> bool {
        self.writable
    }

    pub fn enqueue(&mut self, frame: OutboundFrame) -> WritabilityChange {
        self.bytes_total = self.bytes_total.saturating_add(frame.len());
        self.q.push_back(frame);
        self.recompute_writability()
    }

    fn recompute_writability(&mut self) -> WritabilityChange {
        let prev = self.writable;
        if self.bytes_total >= self.high {
            self.writable = false;
        } else if self.bytes_total <= self.low {
            self.writable = true;
        }
        match (prev, self.writable) {
            (true, false) => WritabilityChange::BecameUnwritable {
                pending_bytes: self.bytes_total,
            },
            (false, true) => WritabilityChange::BecameWritable {
                pending_bytes: self.bytes_total,
            },
            _ => WritabilityChange::None,
        }
    }

    #[inline]
    fn prune_head(&mut self) {
        loop {
            let Some(front) = self.q.front() else {
                self.head_seg = 0;
                self.head_off = 0;
                return;
            };

            // Completed this frame.
            if self.head_seg >= front.seg_count() {
                let _ = self.q.pop_front();
                self.head_seg = 0;
                self.head_off = 0;
                continue;
            }

            let seg_len = front.seg_len(self.head_seg);
            if seg_len == 0 {
                self.head_seg += 1;
                self.head_off = 0;
                continue;
            }

            if self.head_off >= seg_len {
                self.head_seg += 1;
                self.head_off = 0;
                continue;
            }

            break;
        }
    }

    #[inline]
    fn advance_after_write(&mut self, mut written: usize, metas: &[IovMeta]) {
        // `written` is bounded by the sum of gathered iov lengths, which are all non-empty.
        self.bytes_total = self.bytes_total.saturating_sub(written);

        let mut frame_off = 0usize;
        let mut seg = self.head_seg;
        let mut off = self.head_off;

        for m in metas {
            if written == 0 {
                break;
            }

            debug_assert!(m.len > 0);

            if written < m.len {
                frame_off = m.frame_off;
                seg = m.seg;
                off = m.off + written;
                written = 0;
                break;
            }

            // Consumed the full iov slice; advance to the next segment boundary.
            written -= m.len;
            frame_off = m.frame_off;
            seg = m.seg + 1;
            off = 0;
        }

        debug_assert!(written == 0);

        for _ in 0..frame_off {
            let _ = self.q.pop_front();
        }

        if self.q.is_empty() {
            self.head_seg = 0;
            self.head_off = 0;
            return;
        }

        self.head_seg = seg;
        self.head_off = off;
        self.prune_head();
    }

    #[inline]
    fn gather_iov<'a, const N: usize>(
        &'a self,
        max_iov: usize,
        bufs: &mut [&'a [u8]; N],
        metas: &mut [IovMeta; N],
    ) -> usize {
        debug_assert!(max_iov >= 1);
        debug_assert!(max_iov <= N);

        let mut nbuf = 0usize;

        let mut fi = 0usize;
        let mut si = self.head_seg;
        let mut off = self.head_off;

        while nbuf < max_iov {
            let Some(frame) = self.q.get(fi) else {
                break;
            };

            while si < frame.seg_count() && nbuf < max_iov {
                let slice = frame.seg_slice_from(si, off);
                if !slice.is_empty() {
                    bufs[nbuf] = slice;
                    metas[nbuf] = IovMeta {
                        frame_off: fi,
                        seg: si,
                        off,
                        len: slice.len(),
                    };
                    nbuf += 1;
                }

                si += 1;
                off = 0;
            }

            fi += 1;
            si = 0;
            off = 0;
        }

        nbuf
    }

    /// Drain as much as possible to the underlying IO, bounded by a fairness budget.
    ///
    /// Returns `(status, bytes_written, syscalls, writev_calls, writability_change)`.
    pub fn flush_into<I: IoOps + ?Sized>(
        &mut self,
        io: &mut I,
        budget: FlushBudget,
    ) -> (FlushStatus, usize, u64, u64, WritabilityChange) {
        // DECISION: iovec construction must stay stack-only and predictable.
        // `FlushBudget::max_iov` is a product knob (throughput vs. CPU), but it is clamped to
        // `policy::MAX_IOV_CAP` to avoid heap allocation and reduce p99/p999 jitter.
        const IOV_CAP: usize = crate::policy::MAX_IOV_CAP;

        let mut total = 0usize;
        let mut syscalls: u64 = 0;
        let mut writev_calls: u64 = 0;

        // Normalize the head once up-front. Subsequent successful writes always go through
        // `advance_after_write()`, which ends with `prune_head()`, so we avoid re-pruning in the loop.
        self.prune_head();

        while !self.q.is_empty() {
            // Budget check (fairness): do not monopolize the tick.
            if total >= budget.max_bytes || (syscalls as usize) >= budget.max_syscalls {
                let wc = self.recompute_writability();
                return (FlushStatus::Limited, total, syscalls, writev_calls, wc);
            }

            // Build a small iovec (stack-only) to reduce syscalls.
            let max_iov = budget.max_iov.clamp(1, IOV_CAP);
            // DECISION: use `clamp` to make the bounds explicit and clippy-clean.
            // `IOV_CAP` is guaranteed >= 1 (see `policy::MAX_IOV_CAP`), so this cannot panic.
            let mut bufs: [&[u8]; IOV_CAP] = [&[]; IOV_CAP];
            let mut metas: [IovMeta; IOV_CAP] = [IovMeta::EMPTY; IOV_CAP];

            let nbuf = self.gather_iov(max_iov, &mut bufs, &mut metas);

            // Safety: nbuf >= 1 because `prune_head` ensures we have a non-empty head segment.
            debug_assert!(nbuf >= 1);

            syscalls = syscalls.saturating_add(1);
            if nbuf > 1 {
                writev_calls = writev_calls.saturating_add(1);
            }
            let write_res = if nbuf == 1 {
                io.try_write(bufs[0])
            } else {
                io.try_write_vectored(&bufs[..nbuf])
            };

            match write_res {
                Ok(n) => {
                    if n == 0 {
                        let wc = self.recompute_writability();
                        return (FlushStatus::WouldBlock, total, syscalls, writev_calls, wc);
                    }

                    total = total.saturating_add(n);
                    self.advance_after_write(n, &metas[..nbuf]);

                    // Continue flushing until WouldBlock or budget is hit.
                    continue;
                }
                Err(KernelError::WouldBlock) => {
                    let wc = self.recompute_writability();
                    return (FlushStatus::WouldBlock, total, syscalls, writev_calls, wc);
                }
                Err(KernelError::Closed | KernelError::Eof | KernelError::Reset) => {
                    let wc = self.recompute_writability();
                    return (FlushStatus::Closed, total, syscalls, writev_calls, wc);
                }
                Err(_) => {
                    let wc = self.recompute_writability();
                    return (FlushStatus::Error, total, syscalls, writev_calls, wc);
                }
            }
        }

        let wc = self.recompute_writability();
        (FlushStatus::Drained, total, syscalls, writev_calls, wc)
    }
}
