//! Lightweight CRC utilities.
//!
//! Notes:
//! - These implementations are intentionally dependency-free.
//! - For very high throughput use-cases, table-based variants can be added behind a feature.

/// CRC16-CCITT (poly 0x1021), init 0xFFFF.
#[inline]
pub fn crc16_ccitt(bytes: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in bytes {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            if (crc & 0x8000) != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

/// CRC32 (IEEE 802.3, poly 0x04C11DB7), init 0xFFFF_FFFF, reflected.
#[inline]
pub fn crc32_ieee(bytes: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in bytes {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}
