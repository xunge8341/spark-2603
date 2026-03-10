use spark_codec::{ByteToMessageDecoder, DecodeOutcome};
use spark_codec_text::{ascii_box_str, is_token, split_ws_token, CrlfHeadDecoder, TextHeadError, TextHeadLimits};
use spark_core::context::Context;

use super::types::{HttpVersion, RequestHead};

/// Errors produced by [`Http1HeadDecoder`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Http1DecodeError {
    /// The request head exceeded the configured maximum size.
    HeadTooLarge,
    /// The request line was malformed.
    BadRequestLine,
    /// The request head contained an invalid header.
    BadHeader,
    /// The request head contained bytes that violate HTTP token/ASCII rules.
    InvalidEncoding,
}

/// Incremental HTTP/1.1 request-head decoder.
///
/// Production notes:
/// - HTTP/1.x is an *octet-stream* protocol. The request-line and field-names are ASCII.
/// - Field-values may include `obs-text` (bytes >= 0x80); do not assume UTF-8.
///
/// This implements Spark's Netty-style [`ByteToMessageDecoder`] contract.
#[derive(Debug, Clone)]
pub struct Http1HeadDecoder {
    inner: CrlfHeadDecoder,
}

impl Http1HeadDecoder {
    pub fn new(max_head_bytes: usize) -> Self {
        Self::with_limits(max_head_bytes, 64)
    }

    pub fn with_limits(max_head_bytes: usize, max_headers: usize) -> Self {
        // Default per-line limit is generous enough for internal mgmt, strict enough to cap DoS.
        let limits = TextHeadLimits::new(max_head_bytes, max_headers, 8 * 1024);
        Self { inner: CrlfHeadDecoder::new(limits) }
    }
}

impl ByteToMessageDecoder for Http1HeadDecoder {
    type Error = Http1DecodeError;
    type Message = RequestHead;

    fn decode(
        &mut self,
        ctx: &mut Context,
        cumulation: &[u8],
    ) -> Result<DecodeOutcome<Self::Message>, Self::Error> {
        let out = self.inner.decode(ctx, cumulation).map_err(map_text_err)?;
        match out {
            DecodeOutcome::NeedMore => Ok(DecodeOutcome::NeedMore),
            DecodeOutcome::Message { consumed, message } => {
                let (raw, start_line_range, headers) = message.into_parts();
                let start_line = &raw[start_line_range];
                let (method, path, version) = parse_request_line(start_line)?;
                Ok(DecodeOutcome::Message {
                    consumed,
                    message: RequestHead::from_parts(method, path, version, raw, headers),
                })
            }
        }
    }
}

#[inline]
fn map_text_err(e: TextHeadError) -> Http1DecodeError {
    match e {
        TextHeadError::HeadTooLarge => Http1DecodeError::HeadTooLarge,
        TextHeadError::BadStartLine => Http1DecodeError::BadRequestLine,
        TextHeadError::BadHeader | TextHeadError::TooManyHeaders | TextHeadError::LineTooLong => {
            Http1DecodeError::BadHeader
        }
        TextHeadError::InvalidEncoding => Http1DecodeError::InvalidEncoding,
    }
}

fn parse_request_line(line: &[u8]) -> Result<(Box<str>, Box<str>, HttpVersion), Http1DecodeError> {
    // method SP request-target SP HTTP-version
    let (method, rest) = split_ws_token(line).ok_or(Http1DecodeError::BadRequestLine)?;
    let (raw_target, rest) = split_ws_token(rest).ok_or(Http1DecodeError::BadRequestLine)?;
    let (ver, _rest) = split_ws_token(rest).unwrap_or((b"HTTP/1.1", &[][..]));

    if !is_token(method) {
        return Err(Http1DecodeError::BadRequestLine);
    }
    if !raw_target.is_ascii() {
        return Err(Http1DecodeError::InvalidEncoding);
    }
    // Origin-form is what we need for internal mgmt. Be strict to keep the adapter small.
    if !raw_target.starts_with(b"/") {
        return Err(Http1DecodeError::BadRequestLine);
    }

    let version = match ver {
        b"HTTP/1.0" => HttpVersion::Http10,
        b"HTTP/1.1" => HttpVersion::Http11,
        _ => HttpVersion::Http11,
    };

    let target_no_q = raw_target
        .split(|b| *b == b'?')
        .next()
        .unwrap_or(raw_target);

    let method_s = ascii_box_str(method).ok_or(Http1DecodeError::InvalidEncoding)?;
    let path_s = ascii_box_str(target_no_q).ok_or(Http1DecodeError::InvalidEncoding)?;
    Ok((method_s, path_s, version))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_decoder_parses_basic_get() {
        let mut d = Http1HeadDecoder::new(4096);
        let mut ctx = Context::default();
        let req = b"GET /metrics HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let out = d.decode(&mut ctx, req).unwrap();
        match out {
            DecodeOutcome::Message { consumed, message } => {
                assert_eq!(consumed, req.len());
                assert_eq!(message.method.as_ref(), "GET");
                assert_eq!(message.path.as_ref(), "/metrics");
                assert_eq!(message.content_length(), 0);
            }
            DecodeOutcome::NeedMore => panic!("expected message"),
        }
    }

    #[test]
    fn field_value_is_bytes_and_may_be_non_utf8() {
        let mut d = Http1HeadDecoder::new(4096);
        let mut ctx = Context::default();
        let req = b"GET /x HTTP/1.1\r\nX: \xFF\r\n\r\n";
        let out = d.decode(&mut ctx, req).unwrap();
        let DecodeOutcome::Message { message, .. } = out else { panic!("expected message") };
        let v = message.header_bytes("X").unwrap();
        assert_eq!(v, b"\xFF");
        assert!(message.header_ascii("X").is_none());
    }
}
