//! Unsigned LEB128 varints.
//!
//! These helpers are useful for many binary protocols (e.g. protobuf, custom framing)
//! and allow Spark to avoid pulling in external dependencies.

/// Decode an unsigned varint32 from the beginning of `buf`.
///
/// Returns `(value, bytes_consumed)` on success.
#[inline]
pub fn decode_u32(buf: &[u8]) -> Option<(u32, usize)> {
    let mut value: u32 = 0;
    let mut shift: u32 = 0;
    for (i, &b) in buf.iter().take(5).enumerate() {
        value |= ((b & 0x7F) as u32) << shift;
        if (b & 0x80) == 0 {
            return Some((value, i + 1));
        }
        shift += 7;
    }
    None
}

/// Encode an unsigned varint32 into `out`.
///
/// Returns the number of bytes written (1..=5).
#[inline]
pub fn encode_u32(mut v: u32, out: &mut [u8; 5]) -> usize {
    let mut i = 0usize;
    while v >= 0x80 {
        out[i] = ((v as u8) & 0x7F) | 0x80;
        v >>= 7;
        i += 1;
    }
    out[i] = v as u8;
    i + 1
}
