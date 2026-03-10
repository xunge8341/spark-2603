//! HDLC/PPP-like byte stuffing helpers.
//!
//! This is a pragmatic utility for serial/UART links where framing is based on a flag byte
//! (0x7E) and uses escaping (0x7D, XOR 0x20). It is intentionally small and dependency-free.

extern crate alloc;

use alloc::vec::Vec;

const FLAG: u8 = 0x7E;
const ESC: u8 = 0x7D;
const XOR: u8 = 0x20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HdlcError {
    /// Missing a terminating flag byte.
    MissingFlag,
    /// Encountered an escape byte at the end of the frame.
    TruncatedEscape,
}

/// Encode `payload` into `out` using HDLC/PPP-like escaping.
///
/// The resulting frame is: `FLAG` + escaped payload + `FLAG`.
pub fn encode_into(payload: &[u8], out: &mut Vec<u8>) {
    out.push(FLAG);
    for &b in payload {
        match b {
            FLAG | ESC => {
                out.push(ESC);
                out.push(b ^ XOR);
            }
            _ => out.push(b),
        }
    }
    out.push(FLAG);
}

/// Decode an HDLC/PPP-like frame into `out`.
///
/// This function is forgiving about leading flags (it will skip all leading `FLAG` bytes),
/// but it requires a trailing `FLAG` terminator.
pub fn decode_into(frame: &[u8], out: &mut Vec<u8>) -> Result<(), HdlcError> {
    // Skip leading flags.
    let mut i = 0usize;
    while i < frame.len() && frame[i] == FLAG {
        i += 1;
    }
    if i >= frame.len() {
        return Err(HdlcError::MissingFlag);
    }

    let mut esc = false;
    while i < frame.len() {
        let b = frame[i];
        i += 1;

        if esc {
            out.push(b ^ XOR);
            esc = false;
            continue;
        }

        match b {
            FLAG => return Ok(()),
            ESC => {
                if i >= frame.len() {
                    return Err(HdlcError::TruncatedEscape);
                }
                esc = true;
            }
            _ => out.push(b),
        }
    }

    Err(HdlcError::MissingFlag)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_basic() {
        let payload = b"hello";
        let mut enc = Vec::new();
        encode_into(payload, &mut enc);

        let mut dec = Vec::new();
        decode_into(&enc, &mut dec).unwrap();
        assert_eq!(dec, payload);
    }

    #[test]
    fn escapes_flag_and_escape() {
        let payload = [0x01u8, FLAG, 0x02, ESC, 0x03];
        let mut enc = Vec::new();
        encode_into(&payload, &mut enc);

        let mut dec = Vec::new();
        decode_into(&enc, &mut dec).unwrap();
        assert_eq!(dec, payload);
    }
}
