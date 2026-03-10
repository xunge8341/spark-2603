use spark_buffer::Cumulation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::async_bridge::framers) struct ScanState<T: Copy> {
    scanned: usize,
    carry: T,
}

impl<T: Copy> ScanState<T> {
    #[inline]
    pub(in crate::async_bridge::framers) const fn new(carry: T) -> Self {
        Self { scanned: 0, carry }
    }

    #[inline]
    pub(in crate::async_bridge::framers) fn scanned(&self) -> usize {
        self.scanned
    }

    #[inline]
    pub(in crate::async_bridge::framers) fn carry(&self) -> T {
        self.carry
    }

    #[inline]
    pub(in crate::async_bridge::framers) fn reset(&mut self, carry: T) {
        self.scanned = 0;
        self.carry = carry;
    }

    #[inline]
    pub(in crate::async_bridge::framers) fn update(&mut self, scanned: usize, carry: T) {
        self.scanned = scanned;
        self.carry = carry;
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::async_bridge::framers) struct SegmentSlice<'a> {
    pub base: usize,
    pub bytes: &'a [u8],
}

/// Iterates the logical suffix of a segmented byte stream starting at `scanned`, clipped to
/// `scan_limit`.
#[derive(Debug, Clone, Copy)]
pub(in crate::async_bridge::framers) struct SegmentCursor {
    pos: usize,
    skip: usize,
    scan_limit: usize,
}

impl SegmentCursor {
    #[inline]
    pub(in crate::async_bridge::framers) const fn new(scanned: usize, scan_limit: usize) -> Self {
        Self {
            pos: scanned,
            skip: scanned,
            scan_limit,
        }
    }

    #[inline]
    pub(in crate::async_bridge::framers) fn next<'a>(&mut self, seg: &'a [u8]) -> Option<SegmentSlice<'a>> {
        let seg = seg_after_skip(seg, &mut self.skip)?;
        if self.pos >= self.scan_limit {
            return None;
        }

        let scan_seg = clip_to_limit(seg, self.pos, self.scan_limit);
        if scan_seg.is_empty() {
            return None;
        }

        let base = self.pos;
        self.pos = self.pos.saturating_add(scan_seg.len());
        Some(SegmentSlice {
            base,
            bytes: scan_seg,
        })
    }

    #[inline]
    pub(in crate::async_bridge::framers) fn scanned(&self) -> usize {
        self.pos
    }
}

/// Returns a subslice that skips the first `*skip` bytes across segments.
///
/// When the entire segment is skipped, returns `None` and decrements `*skip`.
#[inline]
pub(super) fn seg_after_skip<'a>(seg: &'a [u8], skip: &mut usize) -> Option<&'a [u8]> {
    if *skip == 0 {
        return Some(seg);
    }
    if *skip >= seg.len() {
        *skip -= seg.len();
        return None;
    }

    let s = &seg[*skip..];
    *skip = 0;
    Some(s)
}

/// Clips a segment slice so scanning does not exceed `scan_limit`.
#[inline]
pub(super) fn clip_to_limit(seg: &[u8], base: usize, scan_limit: usize) -> &[u8] {
    let rem = scan_limit.saturating_sub(base);
    if seg.len() > rem {
        &seg[..rem]
    } else {
        seg
    }
}

/// Best-effort peek of the first `n` bytes from cumulation into `out`.
///
/// Returns how many bytes were copied.
#[inline]
pub(super) fn peek_prefix(cumulation: &Cumulation, out: &mut [u8]) -> usize {
    let mut copied = 0usize;
    for seg in cumulation.iter_segments() {
        if copied >= out.len() {
            break;
        }
        let take = (out.len() - copied).min(seg.len());
        out[copied..copied + take].copy_from_slice(&seg[..take]);
        copied += take;
    }
    copied
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_cursor_starts_at_scanned_suffix() {
        let mut cursor = SegmentCursor::new(3, 8);

        assert!(cursor.next(b"ab").is_none());

        let seg = cursor.next(b"cdef").expect("segment after skip");
        assert_eq!(seg.base, 3);
        assert_eq!(seg.bytes, b"def");
        assert_eq!(cursor.scanned(), 6);
    }

    #[test]
    fn segment_cursor_clips_scanned_suffix_to_scan_limit() {
        let mut cursor = SegmentCursor::new(2, 5);
        let seg = cursor.next(b"cdefgh").expect("clipped segment");

        // `scanned` is a logical stream offset, so the cursor first skips two bytes (`cd`)
        // and then clips the remaining suffix to the scan window (`efg`).
        assert_eq!(seg.base, 2);
        assert_eq!(seg.bytes, b"efg");
        assert_eq!(cursor.scanned(), 5);
        assert!(cursor.next(b"tail").is_none());
    }

    #[test]
    fn segment_cursor_stops_at_scan_limit_after_exact_suffix() {
        let mut cursor = SegmentCursor::new(4, 6);

        let seg = cursor.next(b"abcdef").expect("suffix segment");
        assert_eq!(seg.base, 4);
        assert_eq!(seg.bytes, b"ef");
        assert_eq!(cursor.scanned(), 6);
        assert!(cursor.next(b"gh").is_none());
    }
}

