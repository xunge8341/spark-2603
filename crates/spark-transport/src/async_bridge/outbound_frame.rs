use spark_buffer::Bytes;

/// Maximum number of segments carried by an outbound frame.
///
/// DECISION (Milestone-2 / TX symmetry):
/// We cap the frame shape to a small fixed number of segments to keep the hot-path
/// predictable (queue nodes stay small, iovec building stays stack-only), while still
/// enabling **true** zero-copy framing.
///
/// Rationale:
/// - We need up to 3 segments for prefix+payload+suffix.
/// - We reserve a 4th slot to guarantee line/delimiter appends do not require heap allocation
///   even when the upstream already produced a 3-segment frame.
///
/// This keeps delimiter-based protocols "correct by default" without forcing coalescing copies.
pub const OUTBOUND_SEG_MAX: usize = 4;

/// Maximum size for inline (stack/struct) segments.
///
/// This is used for small framing headers (length field / varint32) and bounded delimiters.
pub const OUTBOUND_INLINE_MAX: usize = 16;

/// A small, allocation-free vectored outbound frame.
///
/// Design goals:
/// - Preserve zero-copy for payloads (`Bytes`).
/// - Avoid heap allocation for small framing headers and delimiters.
/// - Keep the shape small and predictable for hot-path queueing.
#[derive(Clone)]
pub struct OutboundFrame {
    segs: [FrameSeg; OUTBOUND_SEG_MAX],
    nsegs: u8,
}

impl core::fmt::Debug for OutboundFrame {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("OutboundFrame")
            .field("nsegs", &self.nsegs)
            .field("len", &self.len())
            .finish()
    }
}

// DECISION: `FrameSeg` defaults to `Empty`. Using `derive(Default)` keeps this
// rule explicit and clippy-clean across refactors.
#[derive(Clone, Default)]
pub enum FrameSeg {
    #[default]
    Empty,
    Bytes(Bytes),
    Inline { buf: [u8; OUTBOUND_INLINE_MAX], len: u8 },
}

impl core::fmt::Debug for FrameSeg {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Empty => f.write_str("Empty"),
            Self::Bytes(b) => f.debug_tuple("Bytes").field(&b.len()).finish(),
            Self::Inline { len, .. } => f.debug_tuple("Inline").field(len).finish(),
        }
    }
}


impl OutboundFrame {
    #[inline]
    pub fn from_bytes(bytes: Bytes) -> Self {
        Self {
            segs: [FrameSeg::Bytes(bytes), FrameSeg::Empty, FrameSeg::Empty, FrameSeg::Empty],
            nsegs: 1,
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub fn seg_count(&self) -> usize {
        self.nsegs as usize
    }

    #[inline]
    pub fn len(&self) -> usize {
        let mut n = 0usize;
        for i in 0..(self.nsegs as usize) {
            n = n.saturating_add(self.segs[i].len());
        }
        n
    }

    #[inline]
    pub fn seg_len(&self, idx: usize) -> usize {
        self.segs.get(idx).map(|s| s.len()).unwrap_or(0)
    }

    #[inline]
    pub fn seg_slice(&self, idx: usize) -> &[u8] {
        self.segs.get(idx).map(|s| s.as_slice()).unwrap_or(&[])
    }

    #[inline]
    pub fn seg_slice_from(&self, idx: usize, off: usize) -> &[u8] {
        self.segs
            .get(idx)
            .map(|s| s.slice_from(off))
            .unwrap_or(&[])
    }

    /// Whether the full frame ends with `suffix`.
    ///
    /// This is used by idempotent line/delimiter encoders.
    pub fn ends_with(&self, suffix: &[u8]) -> bool {
        if suffix.is_empty() {
            return true;
        }

        // Bound: delimiter specs are <= 16 bytes; keep this function stack-only.
        // If a caller asks for a larger suffix, treat it as not-matching.
        const MAX_CHECK: usize = 32;
        if suffix.len() > MAX_CHECK {
            return false;
        }

        let mut buf = [0u8; MAX_CHECK];
        let mut filled = 0usize;

        // Gather last `suffix.len()` bytes from the end across segments.
        let need = suffix.len();
        for i in (0..(self.nsegs as usize)).rev() {
            let s = self.segs[i].as_slice();
            if s.is_empty() {
                continue;
            }
            let mut j = s.len();
            while j > 0 && filled < need {
                j -= 1;
                buf[need - 1 - filled] = s[j];
                filled += 1;
            }
            if filled >= need {
                break;
            }
        }

        if filled < need {
            return false;
        }
        &buf[..need] == suffix
    }

    /// Prepend a small inline segment.
    ///
    /// Returns `None` if `prefix` is too large or the frame has no remaining segment slots.
    pub fn try_prepend_inline(self, prefix: &[u8]) -> Option<Self> {
        if prefix.is_empty() {
            return Some(self);
        }
        if prefix.len() > OUTBOUND_INLINE_MAX {
            return None;
        }
        let n = self.nsegs as usize;
        if n >= OUTBOUND_SEG_MAX {
            return None;
        }

        let mut out = OutboundFrame {
            segs: [FrameSeg::Empty, FrameSeg::Empty, FrameSeg::Empty, FrameSeg::Empty],
            nsegs: (n as u8).saturating_add(1),
        };

        out.segs[0] = FrameSeg::inline(prefix);
        for i in 0..n {
            out.segs[i + 1] = self.segs[i].clone();
        }

        Some(out)
    }

    /// Append a suffix segment, preserving correctness with bounded fallback copying.
    ///
    /// DECISION (Milestone-2 / TX symmetry):
    /// Line/Delimiter encoders must be **correct by default** (always append when missing),
    /// while keeping the hot path allocation-free for the common case.
    ///
    /// Strategy:
    /// 1) Prefer an inline segment (<= OUTBOUND_INLINE_MAX) when a segment slot is available.
    /// 2) Otherwise, append as a `Bytes` segment (allocates only the delimiter bytes).
    /// 3) If the frame is already at the segment cap, coalesce the suffix into the last segment
    ///    (copies only the last segment + suffix; avoids touching earlier payload segments).
    #[inline]
    pub fn append_suffix(mut self, suffix: &[u8]) -> Self {
        if suffix.is_empty() || self.ends_with(suffix) {
            return self;
        }

        let n = self.nsegs as usize;

        // 1) Inline suffix (fast path, no heap).
        if suffix.len() <= OUTBOUND_INLINE_MAX && n < OUTBOUND_SEG_MAX {
            self.segs[n] = FrameSeg::inline(suffix);
            self.nsegs = (n as u8).saturating_add(1);
            return self;
        }

        // 2) Dedicated `Bytes` suffix (allocates only `suffix`).
        if n < OUTBOUND_SEG_MAX {
            self.segs[n] = FrameSeg::Bytes(Bytes::copy_from_slice(suffix));
            self.nsegs = (n as u8).saturating_add(1);
            return self;
        }

        // 3) Segment cap reached: coalesce into the last segment.
        //    This is rare by design (OUTBOUND_SEG_MAX reserves a slot for delimiters),
        //    but it keeps the API semantically correct even for exotic upstream producers.
        if n == 0 {
            // Should not happen, but be total: represent suffix as a single Bytes segment.
            self.segs[0] = FrameSeg::Bytes(Bytes::copy_from_slice(suffix));
            self.nsegs = 1;
            return self;
        }

        let last = n - 1;
        let tail = self.segs[last].as_slice();

        let mut v = std::vec::Vec::with_capacity(tail.len().saturating_add(suffix.len()));
        v.extend_from_slice(tail);
        v.extend_from_slice(suffix);

        self.segs[last] = FrameSeg::Bytes(Bytes::from(v));
        self
    }
}

impl FrameSeg {
    #[inline]
    pub fn len(&self) -> usize {
        match self {
            Self::Empty => 0,
            Self::Bytes(b) => b.len(),
            Self::Inline { len, .. } => *len as usize,
        }
    }

    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        match self {
            Self::Empty => &[],
            Self::Bytes(b) => b.as_slice(),
            Self::Inline { buf, len } => &buf[..(*len as usize)],
        }
    }

    #[inline]
    pub fn slice_from(&self, off: usize) -> &[u8] {
        let s = self.as_slice();
        if off >= s.len() {
            &[]
        } else {
            &s[off..]
        }
    }

    #[inline]
    pub fn inline(bytes: &[u8]) -> Self {
        debug_assert!(bytes.len() <= OUTBOUND_INLINE_MAX);
        let mut buf = [0u8; OUTBOUND_INLINE_MAX];
        buf[..bytes.len()].copy_from_slice(bytes);
        Self::Inline {
            buf,
            len: bytes.len() as u8,
        }
    }
}


impl From<Bytes> for OutboundFrame {
    #[inline]
    fn from(value: Bytes) -> Self {
        Self::from_bytes(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spark_buffer::Bytes;

    #[test]
    fn append_suffix_is_idempotent() {
        // DECISION (protocol correctness): line/delimiter encoders must be idempotent.
        // Appending a suffix that is already present must not mutate the frame shape.
        let f = OutboundFrame::from_bytes(Bytes::from_static(b"abc\n"));
        let f2 = f.clone().append_suffix(b"\n");
        assert_eq!(f.len(), f2.len());
        assert_eq!(f.seg_count(), f2.seg_count());
        assert!(f2.ends_with(b"\n"));
    }

    #[test]
    fn append_suffix_prefers_inline_when_possible() {
        // DECISION (hot path): delimiter/line should not allocate when suffix is small.
        let f = OutboundFrame::from_bytes(Bytes::from_static(b"abc"));
        let f = f.append_suffix(b"\n");
        assert_eq!(f.seg_count(), 2);
        assert_eq!(f.len(), 4);
        assert_eq!(f.seg_slice(1), b"\n");
    }

    #[test]
    fn append_suffix_coalesces_only_tail_when_at_segment_cap() {
        // DECISION (predictability): keep OUTBOUND_SEG_MAX small, but remain correct even if an
        // upstream producer already used all segment slots.
        let f = OutboundFrame::from_bytes(Bytes::from_static(b"payload"))
            .try_prepend_inline(b"P").unwrap() // segs: ["P", payload]
            .append_suffix(b"A")              // segs: ["P", payload, "A"]
            .append_suffix(b"B");             // segs: ["P", payload, "A", "B"]
        assert_eq!(f.seg_count(), OUTBOUND_SEG_MAX);
        assert_eq!(f.seg_slice(3), b"B");

        // Now append one more suffix: must keep seg_count at cap and coalesce only the tail.
        let g = f.clone().append_suffix(b"C");
        assert_eq!(g.seg_count(), OUTBOUND_SEG_MAX);
        assert!(g.ends_with(b"ABC"));
        // Ensure prefix and payload are untouched.
        assert_eq!(g.seg_slice(0), b"P");
        assert_eq!(g.seg_slice(1), b"payload");
    }
}
