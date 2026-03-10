//! Serial / byte-stream framing helpers.
//!
//! These are small wrappers around the generic frame decoders, providing
//! convenience constructors for common link-layer delimiters (SLIP, COBS, HDLC).

mod cobs;
mod hdlc;
mod slip;

pub use cobs::CobsFrameDecoder;
pub use hdlc::HdlcFrameDecoder;
pub use slip::SlipFrameDecoder;
