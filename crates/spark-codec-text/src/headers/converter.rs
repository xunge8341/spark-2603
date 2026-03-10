extern crate alloc;

use alloc::vec::Vec;

/// Typed value conversion error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueConvertError {
    NonAscii,
    InvalidNumber,
    InvalidBool,
}

/// Typed conversion utilities for header values.
///
/// This is inspired by Netty's `ValueConverter` / `CharSequenceValueConverter`,
/// but designed for Spark's byte-based parsing model.
pub trait ValueConverter {
    fn parse_u64(v: &[u8]) -> Result<u64, ValueConvertError>;
    fn format_u64(v: u64, out: &mut Vec<u8>);

    fn parse_bool(v: &[u8]) -> Result<bool, ValueConvertError>;
    fn format_bool(v: bool, out: &mut Vec<u8>);
}

/// Strict ASCII value converter.
///
/// - Numbers are parsed from ASCII decimal.
/// - Bool accepts `"true"/"false"` (case-insensitive) and `"1"/"0"`.
pub struct AsciiValueConverter;

impl AsciiValueConverter {
    /// Parse an ASCII decimal `u64`.
    ///
    /// This is provided as an inherent method so callers don't need to import
    /// the [`ValueConverter`] trait to use dot-call syntax.
    #[inline]
    pub fn parse_u64(v: &[u8]) -> Result<u64, ValueConvertError> {
        <Self as ValueConverter>::parse_u64(v)
    }

    /// Format a `u64` as ASCII decimal.
    #[inline]
    pub fn format_u64(v: u64, out: &mut Vec<u8>) {
        <Self as ValueConverter>::format_u64(v, out)
    }

    /// Parse an ASCII boolean (`true`/`false`/`1`/`0`).
    #[inline]
    pub fn parse_bool(v: &[u8]) -> Result<bool, ValueConvertError> {
        <Self as ValueConverter>::parse_bool(v)
    }

    /// Format a boolean as `true`/`false`.
    #[inline]
    pub fn format_bool(v: bool, out: &mut Vec<u8>) {
        <Self as ValueConverter>::format_bool(v, out)
    }

    /// Parse an ASCII decimal `i64`.
    #[inline]
    pub fn parse_i64(v: &[u8]) -> Result<i64, ValueConvertError> {
        let s = Self::as_ascii(v)?;
        s.trim()
            .parse::<i64>()
            .map_err(|_| ValueConvertError::InvalidNumber)
    }

    /// Format an `i64` as ASCII decimal.
    #[inline]
    pub fn format_i64(v: i64, out: &mut Vec<u8>) {
        if v < 0 {
            out.push(b'-');
            // `wrapping_abs` handles MIN correctly.
            let u = v.wrapping_abs() as u64;
            <Self as ValueConverter>::format_u64(u, out);
        } else {
            <Self as ValueConverter>::format_u64(v as u64, out);
        }
    }

    /// Parse an ASCII decimal `usize`.
    #[inline]
    pub fn parse_usize(v: &[u8]) -> Result<usize, ValueConvertError> {
        let s = Self::as_ascii(v)?;
        s.trim()
            .parse::<usize>()
            .map_err(|_| ValueConvertError::InvalidNumber)
    }

    /// Format a `usize` as ASCII decimal.
    #[inline]
    pub fn format_usize(v: usize, out: &mut Vec<u8>) {
        <Self as ValueConverter>::format_u64(v as u64, out)
    }

    /// Parse an ASCII decimal `u32`.
    #[inline]
    pub fn parse_u32(v: &[u8]) -> Result<u32, ValueConvertError> {
        let s = Self::as_ascii(v)?;
        s.trim()
            .parse::<u32>()
            .map_err(|_| ValueConvertError::InvalidNumber)
    }

    /// Format a `u32` as ASCII decimal.
    #[inline]
    pub fn format_u32(v: u32, out: &mut Vec<u8>) {
        <Self as ValueConverter>::format_u64(v as u64, out)
    }

    #[inline]
    fn as_ascii(v: &[u8]) -> Result<&str, ValueConvertError> {
        if !v.is_ascii() {
            return Err(ValueConvertError::NonAscii);
        }
        core::str::from_utf8(v).map_err(|_| ValueConvertError::NonAscii)
    }
}

impl ValueConverter for AsciiValueConverter {
    fn parse_u64(v: &[u8]) -> Result<u64, ValueConvertError> {
        let s = Self::as_ascii(v)?;
        s.trim()
            .parse::<u64>()
            .map_err(|_| ValueConvertError::InvalidNumber)
    }

    fn format_u64(v: u64, out: &mut Vec<u8>) {
        // Simple itoa-like formatting into a small stack buffer.
        let mut buf = [0u8; 20];
        let mut i = buf.len();
        let mut n = v;
        if n == 0 {
            out.push(b'0');
            return;
        }
        while n > 0 {
            let digit = (n % 10) as u8;
            n /= 10;
            i -= 1;
            buf[i] = b'0' + digit;
        }
        out.extend_from_slice(&buf[i..]);
    }

    fn parse_bool(v: &[u8]) -> Result<bool, ValueConvertError> {
        let s = Self::as_ascii(v)?;
        let s = s.trim();
        if s.eq_ignore_ascii_case("true") || s == "1" {
            return Ok(true);
        }
        if s.eq_ignore_ascii_case("false") || s == "0" {
            return Ok(false);
        }
        Err(ValueConvertError::InvalidBool)
    }

    fn format_bool(v: bool, out: &mut Vec<u8>) {
        out.extend_from_slice(if v { b"true" } else { b"false" });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_parse_u64() {
        assert_eq!(AsciiValueConverter::parse_u64(b" 42 ").unwrap(), 42);
        assert!(matches!(
            AsciiValueConverter::parse_u64(b"x"),
            Err(ValueConvertError::InvalidNumber)
        ));
    }

    #[test]
    fn ascii_parse_bool() {
        assert!(AsciiValueConverter::parse_bool(b"true").unwrap());
        assert!(!AsciiValueConverter::parse_bool(b"FALSE").unwrap());
        assert!(AsciiValueConverter::parse_bool(b"1").unwrap());
        assert!(!AsciiValueConverter::parse_bool(b"0").unwrap());
        assert!(matches!(
            AsciiValueConverter::parse_bool(b"maybe"),
            Err(ValueConvertError::InvalidBool)
        ));
    }
}
