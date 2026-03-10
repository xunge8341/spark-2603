use spark_buffer::{find_byte, Cumulation};

use super::segment::{ScanState, SegmentCursor};
use super::{FrameSpec, StreamDecodeError};

/// Segmented line framer.
///
/// - Searches for `\n` across cumulation segments.
/// - Emits `(consumed, msg_end)` where `msg_end` optionally includes the delimiter.
#[derive(Debug, Clone)]
pub(in crate::async_bridge) struct LineFramer {
    include_delimiter: bool,
    max_len: usize,

    // Incremental scan state: avoid rescanning the entire cumulation when the terminator is not
    // present yet. This prevents O(n^2) behavior under slow/lazy senders.
    scan: ScanState<Option<u8>>,
}

impl LineFramer {
    pub(in crate::async_bridge) fn new(max_len: usize, include_delimiter: bool) -> Self {
        Self {
            include_delimiter,
            max_len: max_len.max(1),
            scan: ScanState::new(None),
        }
    }

    #[inline]
    fn reset_scan(&mut self) {
        self.scan.reset(None);
    }

    pub(in crate::async_bridge) fn decode(
        &mut self,
        cumulation: &Cumulation,
    ) -> core::result::Result<Option<FrameSpec>, StreamDecodeError> {
        let live = cumulation.len();
        if live == 0 {
            return Ok(None);
        }

        // Scan window:
        // - When `include_delimiter == true`, `max_len` bounds the *consumed* bytes (including LF).
        // - When `include_delimiter == false`, allow an extra CRLF (up to 2 bytes) beyond the
        //   message bytes.
        let scan_limit = if self.include_delimiter {
            self.max_len
        } else {
            self.max_len.saturating_add(2)
        };

        if self.scan.scanned() >= scan_limit {
            // Defensive: scan state is relative to the current cumulation; it must never exceed the window.
            self.reset_scan();
            return Err(StreamDecodeError::FrameTooLong);
        }

        let mut cursor = SegmentCursor::new(self.scan.scanned(), scan_limit);
        let mut prev_last = self.scan.carry();

        for seg in cumulation.iter_segments() {
            let Some(seg) = cursor.next(seg) else {
                continue;
            };

            if let Some(i) = find_byte(seg.bytes, b'\n') {
                let nl = seg.base + i;

                // Support both LF and CRLF line endings, including across segment boundaries.
                let before_nl = if i > 0 { Some(seg.bytes[i - 1]) } else { prev_last };
                let has_crlf = before_nl == Some(b'\r');

                let end = if self.include_delimiter {
                    nl + 1
                } else if has_crlf {
                    // Strip the CR as well (CRLF).
                    nl.saturating_sub(1)
                } else {
                    nl
                };

                if end > self.max_len {
                    self.reset_scan();
                    return Err(StreamDecodeError::FrameTooLong);
                }

                self.reset_scan();
                return Ok(Some(FrameSpec {
                    consumed: nl + 1,
                    msg_start: 0,
                    msg_end: end,
                }));
            }

            prev_last = seg.bytes.last().copied();
        }

        // Persist scan progress so the next call only inspects newly buffered bytes.
        self.scan.update(cursor.scanned(), prev_last);

        if live > scan_limit {
            self.reset_scan();
            return Err(StreamDecodeError::FrameTooLong);
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_framer_allows_extra_buffered_bytes_after_newline_within_limit_including_delimiter() {
        let mut c = Cumulation::with_capacity(0);
        c.push_bytes(b"ping\nEXTRA");

        let mut f = LineFramer::new(5, true);
        let spec = f.decode(&c).expect("decode ok").expect("frame");

        assert_eq!(spec.consumed, 5);
        assert_eq!(spec.msg_end, 5);
    }

    #[test]
    fn line_framer_allows_extra_buffered_bytes_after_newline_within_limit_excluding_delimiter() {
        let mut c = Cumulation::with_capacity(0);
        c.push_bytes(b"ping\nEXTRA");

        let mut f = LineFramer::new(4, false);
        let spec = f.decode(&c).expect("decode ok").expect("frame");

        assert_eq!(spec.consumed, 5);
        assert_eq!(spec.msg_end, 4);
    }

    #[test]
    fn line_framer_detects_crlf_across_segment_boundary() {
        let mut c = Cumulation::with_capacity(0);
        c.push_bytes(b"ping\r");
        // Materialize the first push into the immutable queue, so `\n` lands in a new segment.
        c.consume(0);
        c.push_bytes(b"\nEXTRA");

        let mut f = LineFramer::new(4, false);
        let spec = f.decode(&c).expect("decode ok").expect("frame");

        assert_eq!(spec.consumed, 6);
        assert_eq!(spec.msg_end, 4);
    }

    #[test]
    fn line_framer_errors_when_no_newline_within_scan_window() {
        let mut c = Cumulation::with_capacity(0);
        c.push_bytes(b"abcdef");

        let mut f = LineFramer::new(5, true);
        assert!(matches!(f.decode(&c), Err(StreamDecodeError::FrameTooLong)));
    }

    #[test]
    fn line_framer_errors_on_redecode_after_exhausting_scan_window() {
        let mut c = Cumulation::with_capacity(0);
        c.push_bytes(b"abcde");

        let mut f = LineFramer::new(5, true);
        assert!(f.decode(&c).unwrap().is_none());
        assert!(matches!(f.decode(&c), Err(StreamDecodeError::FrameTooLong)));
    }

    #[test]
    fn line_framer_scan_progress_is_monotonic_and_resets_on_success() {
        let mut c = Cumulation::with_capacity(0);
        c.push_bytes(b"abc");

        let mut f = LineFramer::new(8, true);
        assert!(f.decode(&c).unwrap().is_none());
        assert_eq!(f.scan.scanned(), 3);
        assert_eq!(f.scan.carry(), Some(b'c'));

        // Append more bytes; decode should only scan the new suffix and find the terminator.
        c.push_bytes(b"def\nTAIL");
        let spec = f.decode(&c).unwrap().expect("frame");
        assert_eq!(spec.consumed, 7);
        assert_eq!(spec.msg_end, 7);
        assert_eq!(f.scan.scanned(), 0);
        assert_eq!(f.scan.carry(), None);
    }
}
