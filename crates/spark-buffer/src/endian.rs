//! Endianness helpers for binary protocols.
//!
//! These utilities are intentionally small and dependency-free so they can be used
//! from codecs and transports without pulling in external crates.

/// Read a big-endian `u16` from `buf[off..]`.
#[inline]
pub fn read_u16_be_at(buf: &[u8], off: usize) -> Option<u16> {
    let b = buf.get(off..off + 2)?;
    Some(u16::from_be_bytes([b[0], b[1]]))
}

/// Read a little-endian `u16` from `buf[off..]`.
#[inline]
pub fn read_u16_le_at(buf: &[u8], off: usize) -> Option<u16> {
    let b = buf.get(off..off + 2)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

#[inline]
pub fn read_u32_be_at(buf: &[u8], off: usize) -> Option<u32> {
    let b = buf.get(off..off + 4)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

#[inline]
pub fn read_u32_le_at(buf: &[u8], off: usize) -> Option<u32> {
    let b = buf.get(off..off + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Read a 24-bit big-endian unsigned integer from `buf[off..]`.
///
/// Common in industrial/binary protocols.
#[inline]
pub fn read_u24_be_at(buf: &[u8], off: usize) -> Option<u32> {
    let b = buf.get(off..off + 3)?;
    Some(((b[0] as u32) << 16) | ((b[1] as u32) << 8) | (b[2] as u32))
}

/// Read a 24-bit little-endian unsigned integer from `buf[off..]`.
#[inline]
pub fn read_u24_le_at(buf: &[u8], off: usize) -> Option<u32> {
    let b = buf.get(off..off + 3)?;
    Some(((b[2] as u32) << 16) | ((b[1] as u32) << 8) | (b[0] as u32))
}

#[inline]
pub fn read_u64_be_at(buf: &[u8], off: usize) -> Option<u64> {
    let b = buf.get(off..off + 8)?;
    Some(u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}

#[inline]
pub fn read_u64_le_at(buf: &[u8], off: usize) -> Option<u64> {
    let b = buf.get(off..off + 8)?;
    Some(u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}

/// Write a big-endian `u16` to `out[off..]`.
#[inline]
pub fn write_u16_be_at(out: &mut [u8], off: usize, v: u16) -> Option<()> {
    let b = out.get_mut(off..off + 2)?;
    b.copy_from_slice(&v.to_be_bytes());
    Some(())
}

/// Write a little-endian `u16` to `out[off..]`.
#[inline]
pub fn write_u16_le_at(out: &mut [u8], off: usize, v: u16) -> Option<()> {
    let b = out.get_mut(off..off + 2)?;
    b.copy_from_slice(&v.to_le_bytes());
    Some(())
}

#[inline]
pub fn write_u32_be_at(out: &mut [u8], off: usize, v: u32) -> Option<()> {
    let b = out.get_mut(off..off + 4)?;
    b.copy_from_slice(&v.to_be_bytes());
    Some(())
}

#[inline]
pub fn write_u32_le_at(out: &mut [u8], off: usize, v: u32) -> Option<()> {
    let b = out.get_mut(off..off + 4)?;
    b.copy_from_slice(&v.to_le_bytes());
    Some(())
}

/// Write a 24-bit big-endian unsigned integer to `out[off..]`.
///
/// Values above 24 bits are truncated.
#[inline]
pub fn write_u24_be_at(out: &mut [u8], off: usize, v: u32) -> Option<()> {
    let b = out.get_mut(off..off + 3)?;
    b[0] = ((v >> 16) & 0xFF) as u8;
    b[1] = ((v >> 8) & 0xFF) as u8;
    b[2] = (v & 0xFF) as u8;
    Some(())
}

/// Write a 24-bit little-endian unsigned integer to `out[off..]`.
///
/// Values above 24 bits are truncated.
#[inline]
pub fn write_u24_le_at(out: &mut [u8], off: usize, v: u32) -> Option<()> {
    let b = out.get_mut(off..off + 3)?;
    b[0] = (v & 0xFF) as u8;
    b[1] = ((v >> 8) & 0xFF) as u8;
    b[2] = ((v >> 16) & 0xFF) as u8;
    Some(())
}


#[inline]
pub fn write_u64_be_at(out: &mut [u8], off: usize, v: u64) -> Option<()> {
    let b = out.get_mut(off..off + 8)?;
    b.copy_from_slice(&v.to_be_bytes());
    Some(())
}

#[inline]
pub fn write_u64_le_at(out: &mut [u8], off: usize, v: u64) -> Option<()> {
    let b = out.get_mut(off..off + 8)?;
    b.copy_from_slice(&v.to_le_bytes());
    Some(())
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u24_roundtrip() {
        let mut out = [0u8; 3];
        write_u24_be_at(&mut out, 0, 0x12_34_56).unwrap();
        assert_eq!(read_u24_be_at(&out, 0).unwrap(), 0x12_34_56);

        write_u24_le_at(&mut out, 0, 0x12_34_56).unwrap();
        assert_eq!(read_u24_le_at(&out, 0).unwrap(), 0x12_34_56);
    }
}
