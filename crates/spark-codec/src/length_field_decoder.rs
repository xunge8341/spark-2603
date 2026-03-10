use core::ops::Range;

use spark_core::context::Context;

use crate::{ByteToMessageDecoder, DecodeOutcome};

/// Endianness for length fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteOrder {
    Big,
    Little,
}

/// Errors produced by [`LengthFieldBasedFrameDecoder`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthFieldDecodeError {
    /// The frame length exceeded `max_frame_len`.
    FrameTooLong,
    /// The length field or configuration was invalid (negative, underflow, etc.).
    CorruptedLengthField,
    /// Unsupported length-field width.
    UnsupportedLengthFieldLen,
}

/// Netty-style length-field based frame decoder.
///
/// This is an allocation-free decoder that returns a `Range` into the cumulation buffer.
///
/// The derived frame length follows Netty semantics:
/// `frame_len = length_field_end + length_adjustment + length_field_value`.
///
/// See Netty `LengthFieldBasedFrameDecoder` for the conceptual model.
#[derive(Debug, Clone, Copy)]
pub struct LengthFieldBasedFrameDecoder {
    max_frame_len: usize,
    length_field_offset: usize,
    length_field_len: usize,
    length_adjustment: isize,
    initial_bytes_to_strip: usize,
    order: ByteOrder,
}

impl LengthFieldBasedFrameDecoder {
    /// Create a new decoder.
    ///
    /// - `length_field_len` must be one of {1,2,3,4,8}.
    /// - `max_frame_len` is a hard limit for safety.
    pub fn try_new(
        max_frame_len: usize,
        length_field_offset: usize,
        length_field_len: usize,
        length_adjustment: isize,
        initial_bytes_to_strip: usize,
        order: ByteOrder,
    ) -> Result<Self, LengthFieldDecodeError> {
        match length_field_len {
            1 | 2 | 3 | 4 | 8 => {}
            _ => return Err(LengthFieldDecodeError::UnsupportedLengthFieldLen),
        }
        if max_frame_len == 0 {
            return Err(LengthFieldDecodeError::CorruptedLengthField);
        }
        // Basic sanity: we must be able to read the length field.
        let end = length_field_offset.saturating_add(length_field_len);
        if end > max_frame_len {
            return Err(LengthFieldDecodeError::CorruptedLengthField);
        }
        Ok(Self {
            max_frame_len,
            length_field_offset,
            length_field_len,
            length_adjustment,
            initial_bytes_to_strip,
            order,
        })
    }

    #[inline]
    fn read_u24(order: ByteOrder, b: &[u8]) -> u32 {
        debug_assert!(b.len() >= 3);
        match order {
            ByteOrder::Big => ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | (b[2] as u32),
            ByteOrder::Little => ((b[2] as u32) << 16) | ((b[1] as u32) << 8) | (b[0] as u32),
        }
    }

    fn read_len_field(&self, cumulation: &[u8]) -> Result<u64, LengthFieldDecodeError> {
        let off = self.length_field_offset;
        let len = self.length_field_len;
        if cumulation.len() < off + len {
            return Err(LengthFieldDecodeError::CorruptedLengthField);
        }
        let b = &cumulation[off..off + len];
        let v: u64 = match len {
            1 => b[0] as u64,
            2 => match self.order {
                ByteOrder::Big => u16::from_be_bytes([b[0], b[1]]) as u64,
                ByteOrder::Little => u16::from_le_bytes([b[0], b[1]]) as u64,
            },
            3 => Self::read_u24(self.order, b) as u64,
            4 => match self.order {
                ByteOrder::Big => u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as u64,
                ByteOrder::Little => u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as u64,
            },
            8 => match self.order {
                ByteOrder::Big => u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]),
                ByteOrder::Little => u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]),
            },
            _ => return Err(LengthFieldDecodeError::UnsupportedLengthFieldLen),
        };
        Ok(v)
    }

    fn compute_frame_len(&self, len_field_value: u64) -> Result<usize, LengthFieldDecodeError> {
        let end = self.length_field_offset + self.length_field_len;
        let base = end as isize + self.length_adjustment;
        let total = base
            .checked_add(len_field_value as isize)
            .ok_or(LengthFieldDecodeError::CorruptedLengthField)?;
        if total < 0 {
            return Err(LengthFieldDecodeError::CorruptedLengthField);
        }
        let total_usize = total as usize;
        if total_usize < end {
            return Err(LengthFieldDecodeError::CorruptedLengthField);
        }
        if total_usize > self.max_frame_len {
            return Err(LengthFieldDecodeError::FrameTooLong);
        }
        Ok(total_usize)
    }
}

impl ByteToMessageDecoder for LengthFieldBasedFrameDecoder {
    type Error = LengthFieldDecodeError;
    type Message = Range<usize>;

    fn decode(
        &mut self,
        _ctx: &mut Context,
        cumulation: &[u8],
    ) -> Result<DecodeOutcome<Self::Message>, Self::Error> {
        // Need enough bytes to read the length field.
        let end = self.length_field_offset + self.length_field_len;
        if cumulation.len() < end {
            if cumulation.len() > self.max_frame_len {
                return Err(LengthFieldDecodeError::FrameTooLong);
            }
            return Ok(DecodeOutcome::NeedMore);
        }

        let len_field = self.read_len_field(cumulation)?;
        let frame_len = self.compute_frame_len(len_field)?;

        if cumulation.len() < frame_len {
            return Ok(DecodeOutcome::NeedMore);
        }
        if self.initial_bytes_to_strip > frame_len {
            return Err(LengthFieldDecodeError::CorruptedLengthField);
        }

        Ok(DecodeOutcome::Message {
            consumed: frame_len,
            message: self.initial_bytes_to_strip..frame_len,
        })
    }
}
