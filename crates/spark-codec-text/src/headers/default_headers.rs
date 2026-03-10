extern crate alloc;

use alloc::{collections::BTreeMap, vec::Vec};

use crate::HeaderName;

use super::{AsciiValueConverter, HeaderValue, ValueConvertError};

/// One header entry (preserves insertion order).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderEntry {
    pub name: HeaderName,
    pub value: HeaderValue,
}

/// Default header container.
///
/// This is a small, dependency-light equivalent of Netty's `DefaultHeaders`.
/// It preserves insertion order and supports multi-value headers.
///
/// Implementation notes:
/// - `no_std` friendly: uses `alloc::collections::BTreeMap`.
/// - Names are normalized to lowercase ASCII tokens (`HeaderName`).
#[derive(Debug, Clone, Default)]
pub struct DefaultHeaders {
    entries: Vec<HeaderEntry>,
    idx: BTreeMap<HeaderName, Vec<usize>>,
}

impl DefaultHeaders {
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &HeaderEntry> {
        self.entries.iter()
    }

    /// Add a header value (multi-value friendly).
    pub fn add(&mut self, name: HeaderName, value: HeaderValue) {
        let idx = self.entries.len();
        self.entries.push(HeaderEntry { name: name.clone(), value });
        self.idx.entry(name).or_default().push(idx);
    }

    /// Set a header to a single value (removes existing values).
    pub fn set(&mut self, name: HeaderName, value: HeaderValue) {
        self.remove(name.as_str());
        self.add(name, value);
    }

    /// Remove all values for the lowercase header name.
    pub fn remove(&mut self, name_lower: &str) {
        if !self.idx.contains_key(name_lower) {
            return;
        }
        // Stable removal: preserve insertion order for remaining entries.
        self.entries.retain(|e| e.name.as_str() != name_lower);
        self.rebuild_index();
    }

    #[inline]
    pub fn contains(&self, name_lower: &str) -> bool {
        self.idx.contains_key(name_lower)
    }

    /// Get the first value for the lowercase header name.
    pub fn get(&self, name_lower: &str) -> Option<&HeaderValue> {
        let i = self.idx.get(name_lower)?.first().copied()?;
        self.entries.get(i).map(|e| &e.value)
    }

    /// Iterate all values for the lowercase header name.
    pub fn get_all<'a>(&'a self, name_lower: &'a str) -> impl Iterator<Item = &'a HeaderValue> + 'a {
        self.idx
            .get(name_lower)
            .into_iter()
            .flat_map(move |idxs| idxs.iter().filter_map(move |&i| self.entries.get(i).map(|e| &e.value)))
    }

    /// Typed convenience (ASCII): parse the first value as `u64`.
    pub fn get_u64(&self, name_lower: &str) -> Result<Option<u64>, ValueConvertError> {
        let v = match self.get(name_lower) {
            Some(v) => v,
            None => return Ok(None),
        };
        AsciiValueConverter::parse_u64(v.as_bytes()).map(Some)
    }

    /// Typed convenience (ASCII): parse the first value as `u32`.
    pub fn get_u32(&self, name_lower: &str) -> Result<Option<u32>, ValueConvertError> {
        let v = match self.get(name_lower) {
            Some(v) => v,
            None => return Ok(None),
        };
        AsciiValueConverter::parse_u32(v.as_bytes()).map(Some)
    }

    /// Typed convenience (ASCII): parse the first value as `usize`.
    pub fn get_usize(&self, name_lower: &str) -> Result<Option<usize>, ValueConvertError> {
        let v = match self.get(name_lower) {
            Some(v) => v,
            None => return Ok(None),
        };
        AsciiValueConverter::parse_usize(v.as_bytes()).map(Some)
    }

    /// Typed convenience (ASCII): parse the first value as `i64`.
    pub fn get_i64(&self, name_lower: &str) -> Result<Option<i64>, ValueConvertError> {
        let v = match self.get(name_lower) {
            Some(v) => v,
            None => return Ok(None),
        };
        AsciiValueConverter::parse_i64(v.as_bytes()).map(Some)
    }

    /// Typed convenience (ASCII): parse the first value as `bool`.
    pub fn get_bool(&self, name_lower: &str) -> Result<Option<bool>, ValueConvertError> {
        let v = match self.get(name_lower) {
            Some(v) => v,
            None => return Ok(None),
        };
        AsciiValueConverter::parse_bool(v.as_bytes()).map(Some)
    }

    /// Typed convenience (ASCII): set a single `u64` value.
    pub fn set_u64(&mut self, name: HeaderName, v: u64) {
        let mut buf = Vec::new();
        AsciiValueConverter::format_u64(v, &mut buf);
        self.set(name, HeaderValue::from(buf));
    }

    /// Typed convenience (ASCII): set a single `u32` value.
    pub fn set_u32(&mut self, name: HeaderName, v: u32) {
        let mut buf = Vec::new();
        AsciiValueConverter::format_u32(v, &mut buf);
        self.set(name, HeaderValue::from(buf));
    }

    /// Typed convenience (ASCII): set a single `usize` value.
    pub fn set_usize(&mut self, name: HeaderName, v: usize) {
        let mut buf = Vec::new();
        AsciiValueConverter::format_usize(v, &mut buf);
        self.set(name, HeaderValue::from(buf));
    }

    /// Typed convenience (ASCII): set a single `i64` value.
    pub fn set_i64(&mut self, name: HeaderName, v: i64) {
        let mut buf = Vec::new();
        AsciiValueConverter::format_i64(v, &mut buf);
        self.set(name, HeaderValue::from(buf));
    }

    /// Typed convenience (ASCII): set a single `bool` value.
    pub fn set_bool(&mut self, name: HeaderName, v: bool) {
        let mut buf = Vec::new();
        AsciiValueConverter::format_bool(v, &mut buf);
        self.set(name, HeaderValue::from(buf));
    }

    fn rebuild_index(&mut self) {
        let mut idx: BTreeMap<HeaderName, Vec<usize>> = BTreeMap::new();
        for (i, e) in self.entries.iter().enumerate() {
            idx.entry(e.name.clone()).or_default().push(i);
        }
        self.idx = idx;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HeaderName;

    #[test]
    fn add_get_and_remove() {
        let mut h = DefaultHeaders::new();
        let x = HeaderName::from_ascii("x").unwrap();
        h.add(x.clone(), HeaderValue::from_bytes(b"1"));
        h.add(x, HeaderValue::from_bytes(b"2"));
        assert_eq!(h.get("x").unwrap().as_ascii().unwrap(), "1");
        assert_eq!(h.get_all("x").count(), 2);
        h.remove("x");
        assert!(!h.contains("x"));
    }

    #[test]
    fn typed_u64() {
        let mut h = DefaultHeaders::new();
        h.set_u64(HeaderName::from_ascii("content-length").unwrap(), 42);
        assert_eq!(h.get_u64("content-length").unwrap(), Some(42));
    }
}
