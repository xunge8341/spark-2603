extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt;

use spark_buffer::Bytes;
use spark_codec_text::HeaderLine;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SipVersion {
    Sip20,
}

impl SipVersion {
    #[inline]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sip20 => "SIP/2.0",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SipStartLine {
    Request { method: Box<str>, uri: Box<str>, version: SipVersion },
    Response { version: SipVersion, code: u16, reason: Box<str> },
}

impl fmt::Display for SipStartLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SipStartLine::Request { method, uri, version } => {
                write!(f, "{} {} {}", method, uri, version.as_str())
            }
            SipStartLine::Response { version, code, reason } => {
                write!(f, "{} {} {}", version.as_str(), code, reason)
            }
        }
    }
}

/// Parsed SIP message head (start-line + headers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SipHead {
    pub start: SipStartLine,
    raw: Bytes,
    pub headers: Vec<HeaderLine>,
}

impl SipHead {
    #[inline]
    pub fn raw(&self) -> &Bytes {
        &self.raw
    }

    pub fn header_bytes(&self, name: &str) -> Option<&[u8]> {
        let raw = self.raw.as_slice();
        self.headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value_bytes(raw))
    }

    pub fn header_ascii(&self, name: &str) -> Option<&str> {
        let raw = self.raw.as_slice();
        self.headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .and_then(|h| h.value_ascii(raw))
    }

    pub fn content_length(&self) -> usize {
        self.header_ascii("Content-Length")
            .and_then(|v| v.trim().parse::<usize>().ok())
            .unwrap_or(0)
    }

    #[inline]
    pub(crate) fn from_parts(start: SipStartLine, raw: Bytes, headers: Vec<HeaderLine>) -> Self {
        Self { start, raw, headers }
    }
}
