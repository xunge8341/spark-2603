use core::ops::Range;

use spark_core::context::Context;

use crate::{ByteToMessageDecoder, DecodeOutcome};

/// Errors produced by [`DelimiterBasedFrameDecoder`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelimiterDecodeError {
    /// The cumulation exceeded `max_len` without encountering a delimiter.
    FrameTooLong,
}

/// Netty/DotNetty-style delimiter-based stream decoder.
///
/// - Searches the cumulation buffer for the earliest occurrence of any configured delimiter.
/// - Emits the frame as a `Range` into the cumulation (allocation-free).
/// - The adapter removes `consumed` bytes and may call `decode` again to emit more frames.
///
/// This mirrors the intent of Netty's `DelimiterBasedFrameDecoder`, but uses a `Range` message
/// for `no_std` friendliness and to avoid per-frame allocations.
#[derive(Debug, Clone)]
pub struct DelimiterBasedFrameDecoder<'a> {
    delimiters: &'a [&'a [u8]],
    /// Strip the delimiter from the returned frame range.
    strip_delimiter: bool,
    /// Maximum allowed frame length in bytes (including delimiter if `strip_delimiter = false`).
    max_len: usize,
}

impl<'a> DelimiterBasedFrameDecoder<'a> {
    pub fn new(delimiters: &'a [&'a [u8]], max_len: usize, strip_delimiter: bool) -> Self {
        debug_assert!(!delimiters.is_empty());
        Self {
            delimiters,
            strip_delimiter,
            max_len: max_len.max(1),
        }
    }

    #[inline]
    fn find_delim(buf: &[u8], delim: &[u8]) -> Option<usize> {
        let dlen = delim.len();
        if dlen == 0 || buf.len() < dlen {
            return None;
        }

        // Fast path: 1-byte delimiter.
        if dlen == 1 {
            return buf.iter().position(|&b| b == delim[0]);
        }

        // Two-phase scan: find the first byte, then confirm the full delimiter.
        // This avoids a full-window compare at every offset.
        let first = delim[0];
        let last = buf.len() - dlen;
        let mut i = 0usize;
        while i <= last {
            let pos = buf[i..=last].iter().position(|&b| b == first)?;
            let idx = i + pos;
            if &buf[idx..idx + dlen] == delim {
                return Some(idx);
            }
            i = idx + 1;
        }
        None
    }

    fn find_any(&self, buf: &[u8]) -> Option<(usize, usize)> {
        // Return (index, delim_len) for the delimiter that yields the shortest frame.
        let mut best: Option<(usize, usize)> = None;
        for &d in self.delimiters {
            if let Some(i) = Self::find_delim(buf, d) {
                let cand = (i, d.len());
                best = match best {
                    None => Some(cand),
                    Some((bi, bl)) => {
                        let better = i < bi || (i == bi && d.len() < bl);
                        if better {
                            Some(cand)
                        } else {
                            Some((bi, bl))
                        }
                    }
                };
            }
        }
        best
    }
}

impl<'a> ByteToMessageDecoder for DelimiterBasedFrameDecoder<'a> {
    type Error = DelimiterDecodeError;
    type Message = Range<usize>;

    fn decode(
        &mut self,
        _ctx: &mut Context,
        cumulation: &[u8],
    ) -> Result<DecodeOutcome<Self::Message>, Self::Error> {
        let Some((idx, dlen)) = self.find_any(cumulation) else {
            if cumulation.len() > self.max_len {
                return Err(DelimiterDecodeError::FrameTooLong);
            }
            return Ok(DecodeOutcome::NeedMore);
        };

        let end = idx + dlen;
        if end > self.max_len {
            return Err(DelimiterDecodeError::FrameTooLong);
        }

        let message_end = if self.strip_delimiter { idx } else { end };
        Ok(DecodeOutcome::Message {
            consumed: end,
            message: 0..message_end,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::DelimiterBasedFrameDecoder;

    #[test]
    fn find_delim_len1() {
        let buf = b"a,b,c";
        assert_eq!(DelimiterBasedFrameDecoder::find_delim(buf, b","), Some(1));
        assert_eq!(DelimiterBasedFrameDecoder::find_delim(buf, b";"), None);
    }

    #[test]
    fn find_delim_multi_byte() {
        let buf = b"abc\r\nxyz\r\n";
        assert_eq!(DelimiterBasedFrameDecoder::find_delim(buf, b"\r\n"), Some(3));
    }

    #[test]
    fn find_delim_multi_byte_no_full_match_near_end() {
        let buf = b"abc\r";
        assert_eq!(DelimiterBasedFrameDecoder::find_delim(buf, b"\r\n"), None);
    }
}
