use spark_buffer::Cumulation;

use super::segment::peek_prefix;
use super::{FrameSpec, StreamDecodeError};

/// Length-field prefixed framer.
///
/// Wire format: `[len][payload]`.
#[derive(Debug, Clone, Copy)]
pub(in crate::async_bridge) struct LengthFieldPrefixFramer {
    max_len: usize,
    field_len: usize,
    order: spark_codec::ByteOrder,
}

impl LengthFieldPrefixFramer {
    pub(in crate::async_bridge) fn new(max_len: usize, field_len: usize, order: spark_codec::ByteOrder) -> Option<Self> {
        match field_len {
            1 | 2 | 3 | 4 | 8 => {}
            _ => return None,
        }
        Some(Self {
            max_len: max_len.max(1),
            field_len,
            order,
        })
    }

    pub(in crate::async_bridge) fn decode(
        &mut self,
        cumulation: &Cumulation,
    ) -> core::result::Result<Option<FrameSpec>, StreamDecodeError> {
        let live = cumulation.len();
        if live < self.field_len {
            return Ok(None);
        }

        let mut hdr = [0u8; 8];
        let copied = peek_prefix(cumulation, &mut hdr[..self.field_len]);
        if copied < self.field_len {
            return Ok(None);
        }

        let len = read_len_field(self.order, &hdr[..self.field_len])
            .ok_or(StreamDecodeError::CorruptedLengthField)?;
        let payload = len as usize;
        if payload > self.max_len {
            return Err(StreamDecodeError::FrameTooLong);
        }

        let total = self.field_len.saturating_add(payload);
        if live < total {
            return Ok(None);
        }

        Ok(Some(FrameSpec {
            consumed: total,
            msg_start: self.field_len,
            msg_end: total,
        }))
    }
}

#[inline]
fn read_len_field(order: spark_codec::ByteOrder, b: &[u8]) -> Option<u64> {
    match b.len() {
        1 => Some(b[0] as u64),
        2 => Some(match order {
            spark_codec::ByteOrder::Big => u16::from_be_bytes([b[0], b[1]]) as u64,
            spark_codec::ByteOrder::Little => u16::from_le_bytes([b[0], b[1]]) as u64,
        }),
        3 => Some(match order {
            spark_codec::ByteOrder::Big => ((b[0] as u32) << 16 | (b[1] as u32) << 8 | (b[2] as u32)) as u64,
            spark_codec::ByteOrder::Little => {
                ((b[2] as u32) << 16 | (b[1] as u32) << 8 | (b[0] as u32)) as u64
            }
        }),
        4 => Some(match order {
            spark_codec::ByteOrder::Big => u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as u64,
            spark_codec::ByteOrder::Little => u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as u64,
        }),
        8 => Some(match order {
            spark_codec::ByteOrder::Big => u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]),
            spark_codec::ByteOrder::Little => u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]),
        }),
        _ => None,
    }
}
