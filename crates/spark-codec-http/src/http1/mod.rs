//! Minimal HTTP/1.1 codec.
//!
//! This module is intentionally small and focuses on what Spark's internal
//! management plane needs:
//! - parse a request head (request line + headers)
//! - surface `Content-Length`
//! - write a simple `Connection: close` response
//!
//! Encoding policy (production-oriented):
//! - HTTP/1.x is a byte protocol.
//! - request-line tokens and header field-names are ASCII.
//! - header field-values are kept as raw bytes (may contain obs-text >= 0x80).

mod decode;
mod encode;
mod types;
mod util;

pub use decode::{Http1HeadDecoder, Http1DecodeError};
pub use encode::write_response;
pub use types::{Header, HttpVersion, RequestHead};
pub use util::find_header_end;
