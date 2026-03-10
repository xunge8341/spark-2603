use core::ops::Range;

use spark_core::context::Context;

use crate::{delimiters, ByteToMessageDecoder, DecodeOutcome, DelimiterBasedFrameDecoder, DelimiterDecodeError};

/// Convenience frame decoder for COBS-style framing.
///
/// COBS frames are delimited by a `0x00` byte on the wire.
///
/// This decoder extracts raw frame ranges (without the delimiter). Actual COBS
/// decoding (unstuffing) is provided by `spark-buffer::serial::cobs`.
#[derive(Debug, Clone)]
pub struct CobsFrameDecoder {
    inner: DelimiterBasedFrameDecoder<'static>,
}

impl CobsFrameDecoder {
    #[inline]
    pub fn new(max_len: usize) -> Self {
        Self {
            inner: DelimiterBasedFrameDecoder::new(&[delimiters::NUL], max_len, true),
        }
    }
}

impl ByteToMessageDecoder for CobsFrameDecoder {
    type Error = DelimiterDecodeError;
    type Message = Range<usize>;

    #[inline]
    fn decode(
        &mut self,
        ctx: &mut Context,
        cumulation: &[u8],
    ) -> Result<DecodeOutcome<Self::Message>, Self::Error> {
        self.inner.decode(ctx, cumulation)
    }
}
