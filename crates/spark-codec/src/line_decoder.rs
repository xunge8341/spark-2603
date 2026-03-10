use core::ops::Range;

use spark_core::context::Context;

use crate::{ByteToMessageDecoder, DecodeOutcome};

/// Errors produced by [`LineDecoder`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineDecodeError {
    /// A single line exceeded the configured maximum.
    LineTooLong,
}

/// Netty/DotNetty-style line-based stream decoder.
///
/// - Searches for `\n` in the cumulation buffer.
/// - Emits each line (optionally including the delimiter) as a `Range` into the cumulation.
/// - The adapter removes `consumed` bytes and may then call `decode` again to emit more lines.
///
/// This mirrors the core contract of Netty's `LineBasedFrameDecoder`, but keeps the
/// message type allocation-free by returning a byte range instead of a `Vec`.
#[derive(Debug, Clone)]
pub struct LineDecoder {
    /// Include the trailing `\n` in the emitted frame.
    include_delimiter: bool,
    /// Maximum allowed line length in bytes (including delimiter if present).
    max_len: usize,
}

impl LineDecoder {
    /// Create a new line decoder.
    pub fn new(max_len: usize, include_delimiter: bool) -> Self {
        Self { include_delimiter, max_len: max_len.max(1) }
    }

    #[inline]
    fn find_newline(buf: &[u8]) -> Option<usize> {
        // `no_std` friendly scan.
        for (i, b) in buf.iter().enumerate() {
            if *b == b'\n' {
                return Some(i);
            }
        }
        None
    }
}

impl ByteToMessageDecoder for LineDecoder {
    type Error = LineDecodeError;
    type Message = Range<usize>;

    fn decode(
        &mut self,
        _ctx: &mut Context,
        cumulation: &[u8],
    ) -> Result<DecodeOutcome<Self::Message>, Self::Error> {
        let Some(nl) = Self::find_newline(cumulation) else {
            if cumulation.len() > self.max_len {
                return Err(LineDecodeError::LineTooLong);
            }
            return Ok(DecodeOutcome::NeedMore);
        };

        let end = if self.include_delimiter { nl + 1 } else { nl };
        if end > self.max_len {
            return Err(LineDecodeError::LineTooLong);
        }

        Ok(DecodeOutcome::Message { consumed: nl + 1, message: 0..end })
    }
}
