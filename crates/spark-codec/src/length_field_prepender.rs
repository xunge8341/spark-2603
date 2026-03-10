//! Length-field prepender utilities.
//!
//! This module complements [`LengthFieldBasedFrameDecoder`] with symmetric encoding primitives.
//!
//! Unlike Netty's `LengthFieldPrepender`, this implementation is allocation-free:
//! it writes the length field into a caller-provided byte slice.

use crate::length_field_decoder::ByteOrder;

/// Errors produced by [`LengthFieldPrepender`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthFieldPrependerError {
    /// Unsupported length-field width.
    UnsupportedLengthFieldLen,
    /// The provided output slice is too small.
    OutputTooSmall,
    /// Length does not fit into the requested field width.
    LengthOverflow,
}

/// Encode a frame length into a fixed-width length field.
///
/// This is the encoding counterpart to [`crate::LengthFieldBasedFrameDecoder`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LengthFieldPrepender {
    length_field_len: usize,
    order: ByteOrder,
}

impl LengthFieldPrepender {
    #[inline]
    pub fn try_new(length_field_len: usize, order: ByteOrder) -> Result<Self, LengthFieldPrependerError> {
        match length_field_len {
            1 | 2 | 3 | 4 | 8 => Ok(Self { length_field_len, order }),
            _ => Err(LengthFieldPrependerError::UnsupportedLengthFieldLen),
        }
    }

    #[inline]
    pub fn length_field_len(&self) -> usize {
        self.length_field_len
    }

    /// Write the given `len` into `out[..self.length_field_len()]`.
    ///
    /// Returns the number of bytes written (always `length_field_len`).
    pub fn write_len(&self, out: &mut [u8], len: usize) -> Result<usize, LengthFieldPrependerError> {
        if out.len() < self.length_field_len {
            return Err(LengthFieldPrependerError::OutputTooSmall);
        }
        let max = match self.length_field_len {
            1 => u64::from(u8::MAX),
            2 => u64::from(u16::MAX),
            3 => (1u64 << 24) - 1,
            4 => u64::from(u32::MAX),
            8 => u64::MAX,
            _ => return Err(LengthFieldPrependerError::UnsupportedLengthFieldLen),
        };
        let v = len as u64;
        if v > max {
            return Err(LengthFieldPrependerError::LengthOverflow);
        }

        match (self.order, self.length_field_len) {
            (ByteOrder::Big, 1) => out[0] = v as u8,
            (ByteOrder::Little, 1) => out[0] = v as u8,

            (ByteOrder::Big, 2) => {
                out[0] = ((v >> 8) & 0xFF) as u8;
                out[1] = (v & 0xFF) as u8;
            }
            (ByteOrder::Little, 2) => {
                out[0] = (v & 0xFF) as u8;
                out[1] = ((v >> 8) & 0xFF) as u8;
            }

            (ByteOrder::Big, 3) => {
                out[0] = ((v >> 16) & 0xFF) as u8;
                out[1] = ((v >> 8) & 0xFF) as u8;
                out[2] = (v & 0xFF) as u8;
            }
            (ByteOrder::Little, 3) => {
                out[0] = (v & 0xFF) as u8;
                out[1] = ((v >> 8) & 0xFF) as u8;
                out[2] = ((v >> 16) & 0xFF) as u8;
            }

            (ByteOrder::Big, 4) => {
                out[0] = ((v >> 24) & 0xFF) as u8;
                out[1] = ((v >> 16) & 0xFF) as u8;
                out[2] = ((v >> 8) & 0xFF) as u8;
                out[3] = (v & 0xFF) as u8;
            }
            (ByteOrder::Little, 4) => {
                out[0] = (v & 0xFF) as u8;
                out[1] = ((v >> 8) & 0xFF) as u8;
                out[2] = ((v >> 16) & 0xFF) as u8;
                out[3] = ((v >> 24) & 0xFF) as u8;
            }

            (ByteOrder::Big, 8) => {
                for (i, b) in out.iter_mut().take(8).enumerate() {
                    *b = ((v >> (8 * (7 - i))) & 0xFF) as u8;
                }
            }
            (ByteOrder::Little, 8) => {
                for (i, b) in out.iter_mut().take(8).enumerate() {
                    *b = ((v >> (8 * i)) & 0xFF) as u8;
                }
            }
            _ => return Err(LengthFieldPrependerError::UnsupportedLengthFieldLen),
        }

        Ok(self.length_field_len)
    }
}
