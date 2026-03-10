use spark_buffer::{varint, Cumulation};

use super::segment::peek_prefix;
use super::{FrameSpec, StreamDecodeError};

/// Varint32 length-prefixed framer.
#[derive(Debug, Clone, Copy)]
pub(in crate::async_bridge) struct Varint32Framer {
    max_len: usize,
}

impl Varint32Framer {
    pub(in crate::async_bridge) fn new(max_len: usize) -> Self {
        Self {
            max_len: max_len.max(1),
        }
    }

    pub(in crate::async_bridge) fn decode(
        &mut self,
        cumulation: &Cumulation,
    ) -> core::result::Result<Option<FrameSpec>, StreamDecodeError> {
        let live = cumulation.len();
        if live == 0 {
            return Ok(None);
        }

        // Peek up to 5 bytes for the varint prefix.
        let mut hdr = [0u8; 5];
        let copied = peek_prefix(cumulation, &mut hdr);

        let (len, prefix) = match varint::decode_u32(&hdr[..copied]) {
            Some(v) => v,
            None => {
                if copied < 5 {
                    return Ok(None);
                }
                return Err(StreamDecodeError::CorruptedLengthField);
            }
        };

        let payload = len as usize;
        if payload > self.max_len {
            return Err(StreamDecodeError::FrameTooLong);
        }

        let total = prefix.saturating_add(payload);
        if live < total {
            return Ok(None);
        }

        Ok(Some(FrameSpec {
            consumed: total,
            msg_start: prefix,
            msg_end: total,
        }))
    }
}
