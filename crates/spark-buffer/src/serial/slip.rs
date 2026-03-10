//! SLIP (RFC 1055) encode/decode.
//!
//! SLIP is a very small framing mechanism often used for serial links.

extern crate alloc;

use alloc::vec::Vec;

pub const END: u8 = 0xC0;
pub const ESC: u8 = 0xDB;
pub const ESC_END: u8 = 0xDC;
pub const ESC_ESC: u8 = 0xDD;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlipError {
    Malformed,
}

/// Encode `src` as SLIP into `dst`.
///
/// Appends to `dst` and terminates the frame with `END`.
pub fn encode_into(dst: &mut Vec<u8>, src: &[u8]) {
    dst.reserve(src.len() + 2);
    for &b in src {
        match b {
            END => {
                dst.push(ESC);
                dst.push(ESC_END);
            }
            ESC => {
                dst.push(ESC);
                dst.push(ESC_ESC);
            }
            _ => dst.push(b),
        }
    }
    dst.push(END);
}

/// Decode a SLIP frame (without the terminating `END`) into `dst`.
///
/// Appends to `dst`.
pub fn decode_into(dst: &mut Vec<u8>, src: &[u8]) -> Result<(), SlipError> {
    let mut i = 0usize;
    while i < src.len() {
        let b = src[i];
        if b == END {
            // Caller should strip END; tolerate it here as well.
            break;
        }
        if b != ESC {
            dst.push(b);
            i += 1;
            continue;
        }
        // escape sequence
        if i + 1 >= src.len() {
            return Err(SlipError::Malformed);
        }
        let nxt = src[i + 1];
        match nxt {
            ESC_END => dst.push(END),
            ESC_ESC => dst.push(ESC),
            _ => return Err(SlipError::Malformed),
        }
        i += 2;
    }
    Ok(())
}
