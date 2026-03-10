extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;

#[inline]
pub fn ascii_box_str(bytes: &[u8]) -> Option<Box<str>> {
    if !bytes.is_ascii() {
        return None;
    }
    // `Box<str>: From<&str>` is available in `alloc` even in `no_std`.
    // Avoid relying on the `ToOwned` prelude import.
    core::str::from_utf8(bytes).ok().map(Box::<str>::from)
}

#[inline]
pub fn ascii_lower_box_str(bytes: &[u8]) -> Option<Box<str>> {
    if !bytes.is_ascii() {
        return None;
    }
    let mut s = String::with_capacity(bytes.len());
    for &b in bytes {
        s.push((b as char).to_ascii_lowercase());
    }
    Some(s.into_boxed_str())
}

/// Compare a known-lowercase ASCII string with another ASCII string ignoring case.
///
/// This avoids allocating a lowercased copy of `other`.
#[inline]
pub fn eq_ignore_ascii_case_lowered(lowered: &str, other: &str) -> bool {
    let a = lowered.as_bytes();
    let b = other.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    for i in 0..a.len() {
        let bb = b[i];
        if !bb.is_ascii() {
            return false;
        }
        if a[i] != bb.to_ascii_lowercase() {
            return false;
        }
    }
    true
}

/// Trim optional whitespace: OWS = *( SP / HTAB )
#[inline]
pub fn trim_ows(mut s: &[u8]) -> &[u8] {
    while let Some((&b, rest)) = s.split_first() {
        if b == b' ' || b == b'\t' {
            s = rest;
        } else {
            break;
        }
    }
    while let Some((&b, rest)) = s.split_last() {
        if b == b' ' || b == b'\t' {
            s = rest;
        } else {
            break;
        }
    }
    s
}

/// Split and return the next whitespace-delimited token from `s`.
///
/// This is useful for start-line parsing.
#[inline]
pub fn split_ws_token(mut s: &[u8]) -> Option<(&[u8], &[u8])> {
    // skip spaces/tabs
    while let Some((&b, rest)) = s.split_first() {
        if b == b' ' || b == b'\t' {
            s = rest;
        } else {
            break;
        }
    }
    if s.is_empty() {
        return None;
    }
    let mut i = 0usize;
    while i < s.len() {
        let b = s[i];
        if b == b' ' || b == b'\t' {
            break;
        }
        i += 1;
    }
    Some((&s[..i], &s[i..]))
}
