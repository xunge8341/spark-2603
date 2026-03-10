//! Header container utilities (protocol-agnostic).
//!
//! This module is the text-protocol equivalent of Netty's `Headers` / `DefaultHeaders`.
//! It is intentionally **protocol-agnostic**: HTTP, SIP, RTSP, SMTP and other
//! header-based protocols can share the same container, typed accessors and
//! normalization rules.

mod converter;
mod default_headers;
mod value;

pub use converter::{AsciiValueConverter, ValueConvertError, ValueConverter};
pub use default_headers::DefaultHeaders;
/// One header entry (preserves insertion order).
///
/// This is a type alias to keep the module surface stable without relying on a
/// `pub use` that may trigger `unused_imports` under `-D warnings`.
pub type HeaderEntry = default_headers::HeaderEntry;
pub use value::HeaderValue;
