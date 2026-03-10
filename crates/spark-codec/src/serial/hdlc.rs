use core::ops::Range;

use spark_core::context::Context;

use crate::{delimiters, ByteToMessageDecoder, DecodeOutcome, DelimiterBasedFrameDecoder, DelimiterDecodeError};

/// Convenience frame decoder for HDLC/PPP-style flag framing.
///
/// Frames are delimited by `0x7E` flags on the wire. This decoder extracts
/// ranges between flags (without the delimiter). Actual byte-stuffing decode
/// is provided by `spark-buffer::serial::hdlc`.
#[derive(Debug, Clone)]
pub struct HdlcFrameDecoder {
    inner: DelimiterBasedFrameDecoder<'static>,
}

impl HdlcFrameDecoder {
    #[inline]
    pub fn new(max_len: usize) -> Self {
        Self {
            inner: DelimiterBasedFrameDecoder::new(&[delimiters::HDLC_FLAG], max_len, true),
        }
    }
}

impl ByteToMessageDecoder for HdlcFrameDecoder {
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
