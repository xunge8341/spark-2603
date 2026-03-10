extern crate alloc;

use alloc::vec::Vec;
use core::ops::Range;

use spark_buffer::Bytes;
use spark_codec::{ByteToMessageDecoder, DecodeOutcome, DelimiterBasedFrameDecoder, DelimiterDecodeError};
use spark_core::context::Context;

use crate::ascii::{split_ws_token, trim_ows};
use crate::crlf::take_crlf_line;
use crate::header::{HeaderLine, HeaderName};
use crate::limits::TextHeadLimits;
use crate::token::is_token;

const CRLFCRLF_DELIMS: [&[u8]; 1] = [b"\r\n\r\n"];

/// Errors produced by [`CrlfHeadDecoder`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextHeadError {
    HeadTooLarge,
    TooManyHeaders,
    LineTooLong,
    BadStartLine,
    BadHeader,
    InvalidEncoding,
}

/// Parsed start-line + headers, backed by an owned raw buffer.
///
/// This is a protocol-agnostic building block for text-shaped protocols.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextHead {
    raw: Bytes,
    start_line: Range<usize>,
    headers: Vec<HeaderLine>,
}

impl TextHead {
    #[inline]
    pub fn raw(&self) -> &Bytes {
        &self.raw
    }

    #[inline]
    pub fn start_line_bytes(&self) -> &[u8] {
        &self.raw[self.start_line.clone()]
    }

    #[inline]
    pub fn headers(&self) -> &[HeaderLine] {
        &self.headers
    }


    #[inline]
    pub fn start_line_range(&self) -> Range<usize> {
        self.start_line.clone()
    }

    /// Consume the message and return its owned parts.
    #[inline]
    pub fn into_parts(self) -> (Bytes, Range<usize>, Vec<HeaderLine>) {
        (self.raw, self.start_line, self.headers)
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
}

/// CRLF-delimited head decoder (`\r\n\r\n`).
///
/// This is intentionally protocol-agnostic: it only frames and parses header lines.
#[derive(Debug, Clone)]
pub struct CrlfHeadDecoder {
    limits: TextHeadLimits,
    frame: DelimiterBasedFrameDecoder<'static>,
}

impl CrlfHeadDecoder {
    #[inline]
    pub fn new(limits: TextHeadLimits) -> Self {
        let frame = DelimiterBasedFrameDecoder::new(&CRLFCRLF_DELIMS, limits.max_head_bytes, false);
        Self { limits, frame }
    }

    fn parse_head(&self, head: &[u8]) -> Result<TextHead, TextHeadError> {
        // Copy head once and store spans into it.
        let raw = Bytes::copy_from_slice(head);

        let mut pos = 0usize;
        let (start_line, next) = take_crlf_line(head, pos).ok_or(TextHeadError::BadStartLine)?;
        if start_line.len() > self.limits.max_line_bytes {
            return Err(TextHeadError::LineTooLong);
        }
        // Validate start line is ASCII-ish in the sense that it has at least one token.
        // Protocol-specific codecs will do stricter checks.
        if split_ws_token(start_line).is_none() {
            return Err(TextHeadError::BadStartLine);
        }
        let start_line_range = Range { start: pos, end: next - 2 };
        pos = next;

        let mut headers = Vec::new();
        while pos < head.len() {
            let line_start = pos;
            let (line, next) = take_crlf_line(head, pos).ok_or(TextHeadError::BadHeader)?;
            if line.len() > self.limits.max_line_bytes {
                return Err(TextHeadError::LineTooLong);
            }
            pos = next;

            // Empty line terminates the head.
            if line.is_empty() {
                break;
            }

            // Reject obsolete line folding (obs-fold) for safety.
            if matches!(line[0], b' ' | b'\t') {
                return Err(TextHeadError::BadHeader);
            }

            if headers.len() >= self.limits.max_headers {
                return Err(TextHeadError::TooManyHeaders);
            }

            let line_end = next - 2;
            let colon_rel = line.iter().position(|b| *b == b':').ok_or(TextHeadError::BadHeader)?;
            let colon_abs = line_start + colon_rel;

            let name_range = trim_ows_range(head, line_start, colon_abs);
            if name_range.is_empty() || !is_token(&head[name_range.clone()]) {
                return Err(TextHeadError::BadHeader);
            }
            let name = HeaderName::from_token_bytes_lowercase(&head[name_range]).ok_or(TextHeadError::InvalidEncoding)?;

            let value_range = trim_ows_range(head, colon_abs + 1, line_end);
            headers.push(HeaderLine { name, value: value_range });
        }

        Ok(TextHead { raw, start_line: start_line_range, headers })
    }
}

impl ByteToMessageDecoder for CrlfHeadDecoder {
    type Error = TextHeadError;
    type Message = TextHead;

    fn decode(
        &mut self,
        ctx: &mut Context,
        cumulation: &[u8],
    ) -> Result<DecodeOutcome<Self::Message>, Self::Error> {
        match self.frame.decode(ctx, cumulation) {
            Ok(DecodeOutcome::NeedMore) => Ok(DecodeOutcome::NeedMore),
            Ok(DecodeOutcome::Message { consumed, message }) => {
                // `strip_delimiter = false`, so `message` includes the delimiter and matches `consumed`.
                let head = &cumulation[message];
                let msg = self.parse_head(head)?;
                Ok(DecodeOutcome::Message { consumed, message: msg })
            }
            Err(DelimiterDecodeError::FrameTooLong) => Err(TextHeadError::HeadTooLarge),
        }
    }
}

#[inline]
fn trim_ows_range(buf: &[u8], start: usize, end: usize) -> Range<usize> {
    let s = &buf[start..end];
    let trimmed = trim_ows(s);
    let off = trimmed.as_ptr() as usize - s.as_ptr() as usize;
    Range { start: start + off, end: start + off + trimmed.len() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_basic_head_and_headers() {
        let mut d = CrlfHeadDecoder::new(TextHeadLimits::new(4096, 64, 1024));
        let mut ctx = Context::default();
        let msg = b"GET /x HTTP/1.1\r\nHost: a\r\nX: \xFF\r\n\r\n";
        let out = d.decode(&mut ctx, msg).unwrap();
        let DecodeOutcome::Message { message, consumed } = out else { panic!("expected message") };
        assert_eq!(consumed, msg.len());
        assert_eq!(message.header_ascii("Host"), Some("a"));
        assert_eq!(message.header_bytes("X"), Some(b"\xFF".as_slice()));
    }
}
