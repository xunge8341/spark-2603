extern crate alloc;

use alloc::{collections::BTreeMap, vec::Vec};
use core::ops::Range;

use crate::{HeaderLine, HeaderName};

/// A small header index for text protocols (HTTP/SIP/RTSP/SMTP...).
///
/// This type is **protocol-agnostic** and operates on already-parsed header lines.
/// Header names are normalized to lowercase ASCII tokens at parse time.
///
/// Notes:
/// - For `no_std` compatibility we use `BTreeMap` (from `alloc`).
/// - Lookups are performed by the **lowercase** header name. Callers should prefer
///   constants like `"content-length"`.
#[derive(Debug, Clone, Default)]
pub struct HeaderMap {
    idx: BTreeMap<HeaderName, Vec<Range<usize>>>,
}

impl HeaderMap {
    /// Build an index from parsed header lines.
    pub fn from_lines(lines: &[HeaderLine]) -> Self {
        let mut idx: BTreeMap<HeaderName, Vec<Range<usize>>> = BTreeMap::new();
        for h in lines {
            idx.entry(h.name.clone())
                .or_default()
                .push(h.value.clone());
        }
        Self { idx }
    }

    /// Returns the first value as raw bytes for the lowercase header name.
    pub fn first_bytes<'a>(&self, raw: &'a [u8], name_lower: &str) -> Option<&'a [u8]> {
        self.idx
            .get(name_lower)
            .and_then(|v| v.first())
            .map(|r| &raw[r.clone()])
    }

    /// Returns all values (in insertion order) as raw bytes for the lowercase header name.
    pub fn all_bytes<'a>(
        &'a self,
        raw: &'a [u8],
        name_lower: &'a str,
    ) -> impl Iterator<Item = &'a [u8]> + 'a {
        self.idx
            .get(name_lower)
            .into_iter()
            .flat_map(move |ranges| ranges.iter().map(move |r| &raw[r.clone()]))
    }

    /// Returns the first value as ASCII text, if the value is fully ASCII.
    pub fn first_ascii<'a>(&self, raw: &'a [u8], name_lower: &str) -> Option<&'a str> {
        let v = self.first_bytes(raw, name_lower)?;
        if !v.is_ascii() {
            return None;
        }
        core::str::from_utf8(v).ok()
    }

    /// Typed convenience: parse the first ASCII value as `usize`.
    pub fn first_usize(&self, raw: &[u8], name_lower: &str) -> Option<usize> {
        self.first_ascii(raw, name_lower)
            .and_then(|s| s.trim().parse::<usize>().ok())
    }
}
