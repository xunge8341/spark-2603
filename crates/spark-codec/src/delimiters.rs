//! Common delimiter constants.
//!
//! These are small helpers similar in spirit to Netty's `Delimiters` utility.
//! They are protocol-agnostic and can be used with [`crate::DelimiterBasedFrameDecoder`]
//! and related framing primitives.

/// Line-feed (LF) delimiter: `\n`.
pub const LF: &[u8] = b"\n";

/// Carriage-return + line-feed delimiter: `\r\n`.
pub const CRLF: &[u8] = b"\r\n";

/// Double CRLF delimiter: `\r\n\r\n`.
///
/// Commonly used to terminate HTTP/SIP-style header blocks.
pub const DOUBLE_CRLF: &[u8] = b"\r\n\r\n";

/// NUL delimiter: `\0`.
///
/// Useful for COBS-framed streams.
pub const NUL: &[u8] = b"\0";

/// SLIP END delimiter (0xC0).
pub const SLIP_END: &[u8] = b"\xC0";

/// HDLC/PPP flag delimiter (0x7E).
pub const HDLC_FLAG: &[u8] = b"\x7E";
