#![no_std]

//! Text-protocol parsing infrastructure.
//!
//! Spark is a protocol-agnostic communication infrastructure framework. Many control-plane
//! and application protocols are *text-shaped* (HTTP, SIP, RTSP, SMTP...), but they are still
//! **octet-stream** protocols: parsing must be byte-based and must not assume UTF-8.
//!
//! This crate provides small, dependency-light primitives that higher-level codecs can
//! build on (e.g. `spark-codec-http`, `spark-codec-sip`).

extern crate alloc;

mod ascii;
mod crlf;
mod header;
mod header_map;
mod headers;
mod head;
mod limits;
mod token;

pub mod prelude;

pub use header::{HeaderLine, HeaderName};
pub use header_map::HeaderMap;
pub use headers::{
    AsciiValueConverter, DefaultHeaders, HeaderEntry, HeaderValue, ValueConvertError, ValueConverter,
};
pub use ascii::{ascii_box_str, ascii_lower_box_str, split_ws_token, trim_ows};
pub use token::is_token;
pub use head::{CrlfHeadDecoder, TextHead, TextHeadError};
pub use limits::TextHeadLimits;
