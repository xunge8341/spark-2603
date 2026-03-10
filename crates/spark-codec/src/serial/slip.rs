use core::ops::Range;

use spark_core::context::Context;

use crate::{delimiters, ByteToMessageDecoder, DecodeOutcome, DelimiterBasedFrameDecoder, DelimiterDecodeError};

/// Convenience frame decoder for SLIP framing.
///
/// SLIP frames are delimited by a `0xC0` byte on the wire.
///
/// This decoder extracts raw frame ranges (without the delimiter). Actual SLIP
/// decoding (unescaping) is provided by `spark-buffer::serial::slip`.
#[derive(Debug, Clone)]
pub struct SlipFrameDecoder {
    inner: DelimiterBasedFrameDecoder<'static>,
}

impl SlipFrameDecoder {
    #[inline]
    pub fn new(max_len: usize) -> Self {
        Self {
            inner: DelimiterBasedFrameDecoder::new(&[delimiters::SLIP_END], max_len, true),
        }
    }
}

impl ByteToMessageDecoder for SlipFrameDecoder {
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
