use spark_buffer::{find_byte, Cumulation};

use super::super::pipeline::{DelimiterSpec, MAX_DELIMITER_LEN};
use super::segment::{ScanState, SegmentCursor};
use super::{FrameSpec, StreamDecodeError};

/// Delimiter-based segmented framer.
///
/// Uses a small KMP matcher to locate the delimiter across segment boundaries.
#[derive(Debug, Clone)]
pub(in crate::async_bridge) struct DelimiterFramer {
    include_delimiter: bool,
    max_len: usize,
    delim: DelimiterSpec,
    lps: [u8; MAX_DELIMITER_LEN],

    // Incremental scan state: track how many bytes have been scanned so far, and for multi-byte
    // delimiters keep the current KMP matched-prefix length. This keeps decoding O(n) across
    // multiple reads instead of rescanning from the beginning.
    scan: ScanState<usize>,
}

impl DelimiterFramer {
    pub(in crate::async_bridge) fn new(
        max_len: usize,
        delim: DelimiterSpec,
        include_delimiter: bool,
    ) -> Self {
        let mut lps = [0u8; MAX_DELIMITER_LEN];
        let pat = delim.as_slice();

        // Precompute LPS (longest prefix-suffix) table for KMP.
        let mut len = 0usize;
        let mut i = 1usize;
        while i < pat.len() {
            if pat[i] == pat[len] {
                len += 1;
                lps[i] = len as u8;
                i += 1;
            } else if len != 0 {
                len = lps[len - 1] as usize;
            } else {
                lps[i] = 0;
                i += 1;
            }
        }

        Self {
            include_delimiter,
            max_len: max_len.max(1),
            delim,
            lps,
            scan: ScanState::new(0),
        }
    }

    #[inline]
    fn reset_scan(&mut self) {
        self.scan.reset(0);
    }

    pub(in crate::async_bridge) fn decode(
        &mut self,
        cumulation: &Cumulation,
    ) -> core::result::Result<Option<FrameSpec>, StreamDecodeError> {
        let pat = self.delim.as_slice();
        if pat.is_empty() {
            self.reset_scan();
            return Err(StreamDecodeError::InvalidDelimiter);
        }
        let pat_len = self.delim.len();

        let live = cumulation.len();
        if live == 0 {
            return Ok(None);
        }

        // Scan limit: if we've buffered more than max_len + pat_len without finding a delimiter,
        // the frame must be too long.
        let scan_limit = self.max_len.saturating_add(pat_len);

        if self.scan.scanned() > scan_limit {
            self.reset_scan();
            return Err(StreamDecodeError::FrameTooLong);
        }

        // Fast-path: single-byte delimiter.
        if pat_len == 1 {
            let needle = pat[0];
            let mut cursor = SegmentCursor::new(self.scan.scanned(), scan_limit);

            for seg in cumulation.iter_segments() {
                let Some(seg) = cursor.next(seg) else {
                    continue;
                };

                if let Some(i) = find_byte(seg.bytes, needle) {
                    let end = seg.base + i + 1;
                    let start = end.saturating_sub(1);
                    let msg_end = if self.include_delimiter { end } else { start };

                    if msg_end > self.max_len {
                        self.reset_scan();
                        return Err(StreamDecodeError::FrameTooLong);
                    }

                    self.reset_scan();
                    return Ok(Some(FrameSpec {
                        consumed: end,
                        msg_start: 0,
                        msg_end,
                    }));
                }
            }

            self.scan.update(cursor.scanned(), 0);

            if live > scan_limit {
                self.reset_scan();
                return Err(StreamDecodeError::FrameTooLong);
            }
            return Ok(None);
        }

        // Multi-byte delimiter: KMP across segments.
        let mut matched = self.scan.carry().min(pat_len);
        let mut cursor = SegmentCursor::new(self.scan.scanned(), scan_limit);

        for seg in cumulation.iter_segments() {
            let Some(seg) = cursor.next(seg) else {
                continue;
            };

            let mut pos = seg.base;
            for b in seg.bytes.iter() {
                while matched > 0 && *b != pat[matched] {
                    matched = self.lps[matched - 1] as usize;
                }
                if *b == pat[matched] {
                    matched += 1;
                    if matched == pat_len {
                        let end = pos + 1; // exclusive
                        let start = end.saturating_sub(pat_len);
                        let msg_end = if self.include_delimiter { end } else { start };

                        if msg_end > self.max_len {
                            self.reset_scan();
                            return Err(StreamDecodeError::FrameTooLong);
                        }

                        self.reset_scan();
                        return Ok(Some(FrameSpec {
                            consumed: end,
                            msg_start: 0,
                            msg_end,
                        }));
                    }
                }

                pos += 1;
            }
        }

        self.scan.update(cursor.scanned(), matched);

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
    fn delimiter_framer_resumes_kmp_match_across_appends() {
        let delim = DelimiterSpec::new(b"\r\n").unwrap();
        let mut f = DelimiterFramer::new(16, delim, false);

        let mut c = Cumulation::with_capacity(0);
        c.push_bytes(b"abc\r");
        assert!(f.decode(&c).unwrap().is_none());
        assert_eq!(f.scan.scanned(), 4);
        assert_eq!(f.scan.carry(), 1);

        c.push_bytes(b"\nxyz");
        let spec = f.decode(&c).unwrap().expect("frame");
        assert_eq!(spec.consumed, 5);
        assert_eq!(spec.msg_end, 3);
        assert_eq!(f.scan.scanned(), 0);
        assert_eq!(f.scan.carry(), 0);
    }
}
