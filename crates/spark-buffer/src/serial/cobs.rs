//! COBS (Consistent Overhead Byte Stuffing) encode/decode.
//!
//! COBS is commonly used to frame packets over serial links by ensuring a chosen
//! delimiter byte (typically `0x00`) never appears in the encoded payload.

extern crate alloc;

use alloc::vec::Vec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CobsError {
    Malformed,
}

/// Encode `src` as COBS into `dst`.
///
/// This function appends to `dst`.
pub fn encode_into(dst: &mut Vec<u8>, src: &[u8]) {
    // Reserve an upper bound: worst-case overhead is about 1/254.
    dst.reserve(src.len() + src.len() / 254 + 2);

    let start = dst.len();
    dst.push(0); // placeholder for code

    let mut code_index = start;
    let mut code: u8 = 1;

    for &b in src {
        if b == 0 {
            dst[code_index] = code;
            code_index = dst.len();
            dst.push(0); // next code placeholder
            code = 1;
        } else {
            dst.push(b);
            code = code.wrapping_add(1);
            if code == 0xFF {
                dst[code_index] = code;
                code_index = dst.len();
                dst.push(0);
                code = 1;
            }
        }
    }

    dst[code_index] = code;
}

/// Decode a COBS frame from `src` into `dst`.
///
/// This function appends to `dst`.
pub fn decode_into(dst: &mut Vec<u8>, src: &[u8]) -> Result<(), CobsError> {
    let mut i = 0usize;
    while i < src.len() {
        let code = src[i];
        if code == 0 {
            return Err(CobsError::Malformed);
        }
        i += 1;
        let n = (code as usize).saturating_sub(1);
        if i + n > src.len() {
            return Err(CobsError::Malformed);
        }
        dst.extend_from_slice(&src[i..i + n]);
        i += n;
        if code != 0xFF && i < src.len() {
            dst.push(0);
        }
    }
    Ok(())
}
