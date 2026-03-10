extern crate alloc;

use alloc::boxed::Box;
use core::{borrow::Borrow, ops::Range};
use core::{cmp::Ordering, hash::{Hash, Hasher}};

use crate::ascii::{ascii_lower_box_str, eq_ignore_ascii_case_lowered};

/// Normalized header name.
///
/// For text protocols like HTTP/SIP, header names are ASCII tokens and are
/// compared case-insensitively. We normalize to lowercase ASCII to make
/// comparisons predictable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderName(Box<str>);

impl Hash for HeaderName {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Names are normalized to lowercase ASCII.
        self.0.as_bytes().hash(state);
    }
}

impl PartialOrd for HeaderName {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for HeaderName {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl Borrow<str> for HeaderName {
    #[inline]
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl HeaderName {
    /// Construct a normalized header name from an ASCII string.
    ///
    /// The input is lowercased (ASCII) and stored as an owned `Box<str>`.
    /// Returns `None` if the input contains non-ASCII bytes.
    #[inline]
    pub fn from_ascii(name: &str) -> Option<Self> {
        ascii_lower_box_str(name.as_bytes()).map(Self)
    }

    #[inline]
    pub fn new_lower(name_lower: Box<str>) -> Self {
        Self(name_lower)
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        self.0.as_ref()
    }

    #[inline]
    pub fn eq_ignore_ascii_case(&self, other: &str) -> bool {
        eq_ignore_ascii_case_lowered(self.as_str(), other)
    }

    #[inline]
    pub(crate) fn from_token_bytes_lowercase(bytes: &[u8]) -> Option<Self> {
        ascii_lower_box_str(bytes).map(Self)
    }
}

/// A header line parsed from a head block.
///
/// `value` is stored as a byte-range into the `TextHead.raw` buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderLine {
    pub name: HeaderName,
    pub value: Range<usize>,
}

impl HeaderLine {
    #[inline]
    pub fn value_bytes<'a>(&self, raw: &'a [u8]) -> &'a [u8] {
        &raw[self.value.clone()]
    }

    #[inline]
    pub fn value_ascii<'a>(&self, raw: &'a [u8]) -> Option<&'a str> {
        let v = self.value_bytes(raw);
        if !v.is_ascii() {
            return None;
        }
        core::str::from_utf8(v).ok()
    }
}
