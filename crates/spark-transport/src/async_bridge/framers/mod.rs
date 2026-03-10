//! Stream framing primitives.
//!
//! This module contains the built-in stream framers used by `InboundState`.
//! The goal is to keep each framer small, focused, and independently testable.

mod segment;
mod delimiter;
mod length_field;
mod line;
mod varint32;

pub(super) use delimiter::DelimiterFramer;
pub(super) use length_field::LengthFieldPrefixFramer;
pub(super) use line::LineFramer;
pub(super) use varint32::Varint32Framer;

use spark_buffer::Cumulation;

/// Decoded frame boundaries.
///
/// - `consumed`: bytes to remove from cumulation.
/// - `msg_start..msg_end`: message payload slice within the consumed range.
#[derive(Debug, Clone, Copy)]
pub(super) struct FrameSpec {
    pub consumed: usize,
    pub msg_start: usize,
    pub msg_end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StreamDecodeError {
    FrameTooLong,
    InvalidDelimiter,
    CorruptedLengthField,
}

#[derive(Debug, Clone)]
pub(super) enum StreamFramer {
    Line(LineFramer),
    Delimiter(DelimiterFramer),
    LengthField(LengthFieldPrefixFramer),
    Varint32(Varint32Framer),
}

impl StreamFramer {
    pub(super) fn decode(
        &mut self,
        cumulation: &Cumulation,
    ) -> core::result::Result<Option<FrameSpec>, StreamDecodeError> {
        match self {
            StreamFramer::Line(f) => f.decode(cumulation),
            StreamFramer::Delimiter(f) => f.decode(cumulation),
            StreamFramer::LengthField(f) => f.decode(cumulation),
            StreamFramer::Varint32(f) => f.decode(cumulation),
        }
    }
}
