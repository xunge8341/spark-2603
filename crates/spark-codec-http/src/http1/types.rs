use core::fmt;

use spark_buffer::Bytes;
use spark_codec_text::HeaderLine;

/// HTTP header line (name + value range into `RequestHead.raw`).
pub type Header = HeaderLine;

/// HTTP version for request lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpVersion {
    Http10,
    Http11,
}

impl HttpVersion {
    #[inline]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Http10 => "HTTP/1.0",
            Self::Http11 => "HTTP/1.1",
        }
    }
}

/// Parsed HTTP request head (request line + headers).
///
/// Notes:
/// - HTTP/1.x is an *octet-stream* protocol. We store the raw head bytes and keep
///   header values as ranges into that raw buffer.
/// - Header names are ASCII tokens and are compared case-insensitively.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestHead {
    /// Method token (e.g. "GET").
    pub method: Box<str>,
    /// Normalized path (query stripped), e.g. "/metrics".
    pub path: Box<str>,
    pub version: HttpVersion,

    raw: Bytes,
    pub headers: Vec<Header>,
}

impl RequestHead {
    #[inline]
    pub fn raw(&self) -> &Bytes {
        &self.raw
    }

    /// Return the first header value matching `name` (ASCII case-insensitive).
    pub fn header_bytes(&self, name: &str) -> Option<&[u8]> {
        let raw = self.raw.as_slice();
        self.headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value_bytes(raw))
    }

    /// Return the first header value as ASCII `&str` (if ASCII-only).
    pub fn header_ascii(&self, name: &str) -> Option<&str> {
        let raw = self.raw.as_slice();
        self.headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .and_then(|h| h.value_ascii(raw))
    }

    /// Parse `Content-Length` from headers. Missing/invalid values return 0.
    pub fn content_length(&self) -> usize {
        self.header_ascii("Content-Length")
            .and_then(|v| v.trim().parse::<usize>().ok())
            .unwrap_or(0)
    }

    #[inline]
    pub(crate) fn from_parts(
        method: Box<str>,
        path: Box<str>,
        version: HttpVersion,
        raw: Bytes,
        headers: Vec<Header>,
    ) -> Self {
        Self { method, path, version, raw, headers }
    }
}

impl fmt::Display for RequestHead {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} {}", self.method, self.path, self.version.as_str())
    }
}
