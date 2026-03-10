//! Minimal SIP/2.0 head codec.
//!
//! Spark's goal is not to become a SIP framework; instead, we use SIP to validate
//! that the underlying text-codec primitives can support real production protocols.

mod decode;
mod types;

pub use decode::{Sip2HeadDecoder, Sip2DecodeError};
pub use types::{SipHead, SipStartLine, SipVersion};
