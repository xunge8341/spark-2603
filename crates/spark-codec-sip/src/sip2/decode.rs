use spark_codec::{ByteToMessageDecoder, DecodeOutcome};
use spark_codec_text::{ascii_box_str, is_token, split_ws_token, CrlfHeadDecoder, TextHeadError, TextHeadLimits};
use spark_core::context::Context;

use super::types::{SipHead, SipStartLine, SipVersion};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sip2DecodeError {
    HeadTooLarge,
    BadStartLine,
    BadHeader,
    InvalidEncoding,
}

#[derive(Debug, Clone)]
pub struct Sip2HeadDecoder {
    inner: CrlfHeadDecoder,
}

impl Sip2HeadDecoder {
    pub fn new(max_head_bytes: usize) -> Self {
        Self::with_limits(max_head_bytes, 64)
    }

    pub fn with_limits(max_head_bytes: usize, max_headers: usize) -> Self {
        let limits = TextHeadLimits::new(max_head_bytes, max_headers, 8 * 1024);
        Self { inner: CrlfHeadDecoder::new(limits) }
    }
}

impl ByteToMessageDecoder for Sip2HeadDecoder {
    type Error = Sip2DecodeError;
    type Message = SipHead;

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
                let start = parse_start_line(start_line)?;
                Ok(DecodeOutcome::Message {
                    consumed,
                    message: SipHead::from_parts(start, raw, headers),
                })
            }
        }
    }
}

#[inline]
fn map_text_err(e: TextHeadError) -> Sip2DecodeError {
    match e {
        TextHeadError::HeadTooLarge => Sip2DecodeError::HeadTooLarge,
        TextHeadError::BadStartLine => Sip2DecodeError::BadStartLine,
        TextHeadError::BadHeader | TextHeadError::TooManyHeaders | TextHeadError::LineTooLong => {
            Sip2DecodeError::BadHeader
        }
        TextHeadError::InvalidEncoding => Sip2DecodeError::InvalidEncoding,
    }
}

fn parse_start_line(line: &[u8]) -> Result<SipStartLine, Sip2DecodeError> {
    let (t1, rest) = split_ws_token(line).ok_or(Sip2DecodeError::BadStartLine)?;
    if t1 == b"SIP/2.0" {
        // Status-Line: SIP-Version SP Status-Code SP Reason-Phrase
        let (code_b, rest) = split_ws_token(rest).ok_or(Sip2DecodeError::BadStartLine)?;
        if !code_b.is_ascii() {
            return Err(Sip2DecodeError::InvalidEncoding);
        }
        let code_s = core::str::from_utf8(code_b).map_err(|_| Sip2DecodeError::InvalidEncoding)?;
        let code: u16 = code_s.parse().map_err(|_| Sip2DecodeError::BadStartLine)?;

        // Reason-Phrase: remainder, trim leading whitespace.
        let mut i = 0usize;
        while i < rest.len() && (rest[i] == b' ' || rest[i] == b'\t') {
            i += 1;
        }
        let reason_b = &rest[i..];
        let reason = ascii_box_str(reason_b).ok_or(Sip2DecodeError::InvalidEncoding)?;
        Ok(SipStartLine::Response { version: SipVersion::Sip20, code, reason })
    } else {
        // Request-Line: Method SP Request-URI SP SIP-Version
        if !is_token(t1) {
            return Err(Sip2DecodeError::BadStartLine);
        }
        let (uri_b, rest) = split_ws_token(rest).ok_or(Sip2DecodeError::BadStartLine)?;
        let (ver_b, _rest) = split_ws_token(rest).ok_or(Sip2DecodeError::BadStartLine)?;
        if ver_b != b"SIP/2.0" {
            return Err(Sip2DecodeError::BadStartLine);
        }
        if !uri_b.is_ascii() {
            return Err(Sip2DecodeError::InvalidEncoding);
        }
        let method = ascii_box_str(t1).ok_or(Sip2DecodeError::InvalidEncoding)?;
        let uri = ascii_box_str(uri_b).ok_or(Sip2DecodeError::InvalidEncoding)?;
        Ok(SipStartLine::Request { method, uri, version: SipVersion::Sip20 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_invite() {
        let mut d = Sip2HeadDecoder::new(4096);
        let mut ctx = Context::default();
        let msg = b"INVITE sip:bob@example.com SIP/2.0\r\nContent-Length: 0\r\n\r\n";
        let out = d.decode(&mut ctx, msg).unwrap();
        let DecodeOutcome::Message { message, .. } = out else { panic!("expected message") };
        match &message.start {
            SipStartLine::Request { method, uri, version } => {
                assert_eq!(method.as_ref(), "INVITE");
                assert_eq!(uri.as_ref(), "sip:bob@example.com");
                assert_eq!(*version, SipVersion::Sip20);
            }
            _ => panic!("expected request"),
        }
        assert_eq!(message.content_length(), 0);
    }
}
