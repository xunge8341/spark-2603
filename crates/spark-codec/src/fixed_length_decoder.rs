use core::ops::Range;

use spark_core::context::Context;

use crate::{ByteToMessageDecoder, DecodeOutcome};

/// Errors produced by [`FixedLengthFrameDecoder`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixedLengthDecodeError {
    /// The configured frame length was zero.
    InvalidFrameLen,
}

/// Netty-style fixed-length frame decoder.
///
/// Emits a frame every time the cumulation buffer holds at least `frame_len` bytes.
/// The emitted message is a `Range` into the cumulation buffer; the adapter is
/// responsible for dropping `consumed` bytes and calling `decode` again.
#[derive(Debug, Clone, Copy)]
pub struct FixedLengthFrameDecoder {
    frame_len: usize,
}

impl FixedLengthFrameDecoder {
    /// Create a new fixed-length decoder.
    ///
    /// Returns an error if `frame_len == 0`.
    pub fn try_new(frame_len: usize) -> Result<Self, FixedLengthDecodeError> {
        if frame_len == 0 {
            return Err(FixedLengthDecodeError::InvalidFrameLen);
        }
        Ok(Self { frame_len })
    }
    /// Create a new decoder with a non-zero frame length.
    #[inline]
    pub fn new(frame_len: core::num::NonZeroUsize) -> Self {
        Self { frame_len: frame_len.get() }
    }

    #[inline]
    pub fn frame_len(&self) -> usize {
        self.frame_len
    }
}

impl ByteToMessageDecoder for FixedLengthFrameDecoder {
    type Error = FixedLengthDecodeError;
    type Message = Range<usize>;

    fn decode(
        &mut self,
        _ctx: &mut Context,
        cumulation: &[u8],
    ) -> Result<DecodeOutcome<Self::Message>, Self::Error> {
        if cumulation.len() < self.frame_len {
            return Ok(DecodeOutcome::NeedMore);
        }
        Ok(DecodeOutcome::Message { consumed: self.frame_len, message: 0..self.frame_len })
    }
}
