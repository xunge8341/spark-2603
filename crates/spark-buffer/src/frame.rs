//! Helpers for building framed binary messages.

extern crate alloc;

use alloc::vec::Vec;

/// Append a u32 length prefix in big-endian followed by `payload`.
///
/// This is a convenience for length-field framed protocols.
pub fn append_u32_be_len_prefixed(out: &mut Vec<u8>, payload: &[u8]) {
    let len = payload.len() as u32;
    let mut tmp = [0u8; 4];
    tmp.copy_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&tmp);
    out.extend_from_slice(payload);
}

/// Append a u32 length prefix in little-endian followed by `payload`.
pub fn append_u32_le_len_prefixed(out: &mut Vec<u8>, payload: &[u8]) {
    let len = payload.len() as u32;
    let mut tmp = [0u8; 4];
    tmp.copy_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&tmp);
    out.extend_from_slice(payload);
}
