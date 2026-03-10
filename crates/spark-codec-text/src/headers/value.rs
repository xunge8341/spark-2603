extern crate alloc;

use alloc::{boxed::Box, vec::Vec};

/// Owned header value.
///
/// Protocol note:
/// - For HTTP/1.x and SIP, header values are **octet sequences** and may contain
///   `obs-text` bytes (>= 0x80). Therefore we store values as raw bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderValue(Box<[u8]>);

impl HeaderValue {
    #[inline]
    pub fn from_bytes(v: &[u8]) -> Self {
        Self(Box::<[u8]>::from(v))
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Returns an ASCII view of this value if it is fully ASCII.
    #[inline]
    pub fn as_ascii(&self) -> Option<&str> {
        if !self.0.is_ascii() {
            return None;
        }
        core::str::from_utf8(&self.0).ok()
    }

    #[inline]
    pub fn into_vec(self) -> Vec<u8> {
        self.0.into_vec()
    }
}

impl From<Vec<u8>> for HeaderValue {
    #[inline]
    fn from(v: Vec<u8>) -> Self {
        Self(v.into_boxed_slice())
    }
}

impl From<&[u8]> for HeaderValue {
    #[inline]
    fn from(v: &[u8]) -> Self {
        Self::from_bytes(v)
    }
}
