use crate::{ByteQueue, Bytes, BytesMut, BytesMutAllocEvidence};

/// Stats for extracting a frame from the cumulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TakeStats {
    /// Bytes copied due to coalescing multi-segment frames.
    pub copied_bytes: usize,
    /// Whether this extraction required coalescing.
    pub coalesced: bool,
}

/// TCP/stream cumulation buffer.
///
/// This is a segmented cumulation designed to minimize large memmoves on the
/// read path. Data is stored as a FIFO queue of immutable segments.
///
/// Design constraints:
/// - Keep the API small and explicit.
/// - Prefer returning `Bytes` views when a frame is contained in a single segment.
/// - Preserve correctness for multi-segment frames by coalescing only when needed.
#[derive(Debug, Default)]
pub struct Cumulation {
    q: ByteQueue,

    // Tail accumulation buffer.
    //
    // DECISION (BigStep-20B): keep the read-path segmented, but accumulate newly read bytes into a
    // single mutable tail buffer to reduce segment fragmentation.
    // - `ByteQueue::push_slice` allocates a new `Bytes` per read, which can explode segment counts
    //   under small-read workloads and increases the probability of multi-segment frames.
    // - A single `BytesMut` tail keeps appends contiguous; the decoder sees it as one extra segment.
    // - Consumption uses `BytesMut`'s bounded compaction heuristics; we do not re-introduce large
    //   memmoves on the hot path.
    tail: BytesMut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CumulationAllocEvidence {
    pub tail_capacity_growth_count: u64,
    pub tail_peak_capacity: usize,
}

impl Cumulation {
    /// Create a new cumulation.
    ///
    /// The capacity hint is used for the tail buffer, which amortizes allocations under small-read
    /// workloads.
    #[inline]
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            q: ByteQueue::new(),
            tail: BytesMut::with_capacity(cap),
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.q.len().saturating_add(self.tail.len())
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.q.is_empty() && self.tail.is_empty()
    }

    #[inline]
    pub fn iter_segments(&self) -> impl Iterator<Item = &[u8]> {
        let tail = self.tail.as_slice();
        self.q
            .iter_segments()
            .chain(core::iter::once(tail).filter(|s| !s.is_empty()))
    }

    #[inline]
    pub fn push_bytes(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.tail.extend_from_slice(bytes);
    }

    #[inline]
    pub fn alloc_evidence(&self) -> CumulationAllocEvidence {
        let BytesMutAllocEvidence {
            capacity_growth_count,
            peak_capacity,
        } = self.tail.alloc_evidence();
        CumulationAllocEvidence {
            tail_capacity_growth_count: capacity_growth_count,
            tail_peak_capacity: peak_capacity,
        }
    }

    #[inline]
    pub fn consume(&mut self, n: usize) {
        self.materialize_tail();
        self.q.advance(n);
    }

    /// Extract a decoded message from the cumulation and consume `consumed` bytes.
    ///
    /// `msg_end` is the length of the message bytes within the consumed prefix.
    ///
    /// - If the message is fully contained in the head segment, returns a zero-copy `Bytes` view.
    /// - Otherwise, coalesces only the message bytes.
    pub fn take_message_with_stats(
        &mut self,
        consumed: usize,
        msg_end: usize,
    ) -> (Bytes, TakeStats) {
        self.take_range_with_stats(consumed, 0, msg_end)
    }

    /// Extract a decoded message range from the cumulation and consume `consumed` bytes.
    ///
    /// `msg_start..msg_end` refers to the message bytes within the consumed prefix.
    ///
    /// This is required for binary protocols that carry metadata in front of the payload
    /// (e.g. length prefixes) where the decoder strips the header.
    pub fn take_range_with_stats(
        &mut self,
        consumed: usize,
        msg_start: usize,
        msg_end: usize,
    ) -> (Bytes, TakeStats) {
        // Once we start consuming bytes (as opposed to scanning), materialize the tail into the
        // immutable segment queue.
        //
        // This keeps the hot-path consumption zero-copy (slice) and avoids large memmoves that a
        // purely-contiguous cumulation would incur.
        self.materialize_tail();

        let live = self.len();
        let consumed = consumed.min(live);

        let msg_start = msg_start.min(consumed);
        let msg_end = msg_end.min(consumed);

        if msg_end <= msg_start {
            // No message bytes; drop the consumed prefix.
            if consumed > 0 {
                self.consume(consumed);
            }
            return (Bytes::from_static(b""), TakeStats::default());
        }

        // Drop the prefix before the message.
        if msg_start > 0 {
            self.consume(msg_start);
        }

        let msg_len = msg_end - msg_start;

        // Fast-path check: whether the message is contained in the current head segment.
        let single_seg = self
            .q
            .peek_contiguous()
            .map(|s| s.len() >= msg_len)
            .unwrap_or(false);

        let msg = self
            .q
            .pop_exact(msg_len)
            .unwrap_or_else(|| Bytes::from_static(b""));

        // Drop the remaining bytes within the consumed prefix.
        let tail_drop = consumed.saturating_sub(msg_end);
        if tail_drop > 0 {
            self.consume(tail_drop);
        }

        let stats = if single_seg {
            TakeStats::default()
        } else {
            TakeStats {
                copied_bytes: msg_len,
                coalesced: true,
            }
        };

        (msg, stats)
    }

    #[inline]
    fn materialize_tail(&mut self) {
        if self.tail.is_empty() {
            return;
        }
        let b = self.tail.freeze();
        self.q.push(b);
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;

    use super::Cumulation;
    use alloc::vec;
    use alloc::vec::Vec;

    #[test]
    fn tail_accumulates_multiple_pushes_as_single_segment() {
        let mut c = Cumulation::with_capacity(0);
        c.push_bytes(b"ab");
        c.push_bytes(b"cd");
        c.push_bytes(b"ef");

        let segs: Vec<Vec<u8>> = c.iter_segments().map(|s| s.to_vec()).collect();
        assert_eq!(segs, vec![b"abcdef".to_vec()]);

        let (m, stats) = c.take_message_with_stats(6, 6);
        assert_eq!(m.as_ref(), b"abcdef");
        assert!(!stats.coalesced);
        assert_eq!(stats.copied_bytes, 0);
        assert!(c.is_empty());
    }

    #[test]
    fn queued_remainder_plus_tail_can_coalesce_on_take() {
        let mut c = Cumulation::with_capacity(0);
        c.push_bytes(b"abc");
        let (m1, s1) = c.take_message_with_stats(2, 2);
        assert_eq!(m1.as_ref(), b"ab");
        assert!(!s1.coalesced);

        // Remainder stays in the queue; new bytes land in the tail until the next take.
        c.push_bytes(b"def");
        let (m2, s2) = c.take_message_with_stats(4, 4);
        assert_eq!(m2.as_ref(), b"cdef");
        assert!(s2.coalesced);
        assert_eq!(s2.copied_bytes, 4);
    }

    #[test]
    fn alloc_evidence_exposes_tail_growth_metrics() {
        let mut c = Cumulation::with_capacity(1);
        c.push_bytes(&[0u8; 1024]);
        c.push_bytes(&[0u8; 1024]);
        let ev = c.alloc_evidence();
        assert!(ev.tail_capacity_growth_count > 0);
        assert!(ev.tail_peak_capacity >= 2048);
    }
}
