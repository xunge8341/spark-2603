//! Protobuf-style varint32 length framing.
//!
//! This module provides the allocation-free equivalents of Netty's
//! `ProtobufVarint32FrameDecoder` and `ProtobufVarint32LengthFieldPrepender`.

use core::ops::Range;

use spark_core::context::Context;

use crate::{ByteToMessageDecoder, DecodeOutcome};

/// Errors produced by varint32 framing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Varint32Error {
    /// The varint prefix is malformed or exceeds 5 bytes.
    MalformedVarint,
    /// The derived frame length exceeded the configured maximum.
    FrameTooLong,
}

fn decode_varint32_prefix(buf: &[u8]) -> Result<Option<(u32, usize)>, Varint32Error> {
    // Protobuf varint32 is at most 5 bytes.
    let mut value: u32 = 0;
    let mut shift: u32 = 0;

    for (i, &b) in buf.iter().take(5).enumerate() {
        let bits = (b & 0x7F) as u32;
        value |= bits << shift;
        if (b & 0x80) == 0 {
            return Ok(Some((value, i + 1)));
        }
        shift += 7;
    }

    // If we consumed 5 bytes and didn't terminate, it's malformed.
    if buf.len() >= 5 {
        Err(Varint32Error::MalformedVarint)
    } else {
        Ok(None)
    }
}

/// Decode frames prefixed with a protobuf varint32 length field.
///
/// The produced message is a `Range` into the cumulation which points to the
/// payload bytes *excluding* the varint prefix.
#[derive(Debug, Clone, Copy)]
pub struct ProtobufVarint32FrameDecoder {
    max_frame_len: usize,
}

impl ProtobufVarint32FrameDecoder {
    #[inline]
    pub fn new(max_frame_len: usize) -> Self {
        Self { max_frame_len }
    }
}

impl ByteToMessageDecoder for ProtobufVarint32FrameDecoder {
    type Error = Varint32Error;
    type Message = Range<usize>;

    fn decode(
        &mut self,
        _ctx: &mut Context,
        cumulation: &[u8],
    ) -> Result<DecodeOutcome<Self::Message>, Self::Error> {
        let Some((len, prefix_len)) = decode_varint32_prefix(cumulation)? else {
            return Ok(DecodeOutcome::NeedMore);
        };
        let frame_len = len as usize;
        if frame_len > self.max_frame_len {
            return Err(Varint32Error::FrameTooLong);
        }
        let total = prefix_len + frame_len;
        if cumulation.len() < total {
            return Ok(DecodeOutcome::NeedMore);
        }
        let range = Range { start: prefix_len, end: total };
        Ok(DecodeOutcome::Message { consumed: total, message: range })
    }
}

/// Encode a protobuf varint32 length field.
///
/// This is allocation-free; it writes the encoded varint into a caller-provided
/// buffer (at most 5 bytes).
#[derive(Debug, Clone, Copy, Default)]
pub struct ProtobufVarint32LengthFieldPrepender;

impl ProtobufVarint32LengthFieldPrepender {
    /// Write varint32 `len` to `out` and return the number of bytes written.
    pub fn write_len(out: &mut [u8; 5], len: u32) -> usize {
        let mut v = len;
        let mut i = 0usize;
        while v >= 0x80 {
            out[i] = ((v as u8) & 0x7F) | 0x80;
            v >>= 7;
            i += 1;
        }
        out[i] = v as u8;
        i + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spark_core::context::Context;

    #[test]
    fn varint32_roundtrip_small() {
        let mut buf = [0u8; 5];
        let n = ProtobufVarint32LengthFieldPrepender::write_len(&mut buf, 150);
        assert_eq!(n, 2);

        let mut dec = ProtobufVarint32FrameDecoder::new(1024);
        let mut ctx = Context::default();

        // frame length 3, prefix is 1 byte.
        let mut frame = [0u8; 16];
        let p = ProtobufVarint32LengthFieldPrepender::write_len(&mut buf, 3);
        frame[..p].copy_from_slice(&buf[..p]);
        frame[p..p + 3].copy_from_slice(b"abc");

        let out = dec.decode(&mut ctx, &frame[..p + 3]).unwrap();
        match out {
            DecodeOutcome::Message { consumed, message } => {
                assert_eq!(consumed, p + 3);
                assert_eq!(&frame[message], b"abc");
            }
            _ => panic!("expected message"),
        }
    }
}
