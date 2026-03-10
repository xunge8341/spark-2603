//! Prelude exports for the codec layer.
//!
//! Spark codecs are intentionally trait-driven (Netty-style: `ByteToMessageDecoder`, etc.).
//! Bringing this prelude into scope makes common trait methods available via dot-call syntax.

pub use crate::byte_to_message::{ByteToMessageDecoder, DecodeOutcome};
pub use crate::delimiter_decoder::{DelimiterBasedFrameDecoder, DelimiterDecodeError};
pub use crate::delimiter_encoder::{DelimiterEncodeError, DelimiterEncoder};
pub use crate::delimiters;
pub use crate::decoder::Decoder;
pub use crate::encoder::Encoder;
pub use crate::intent::Intent;
pub use crate::line_decoder::{LineDecodeError, LineDecoder};
pub use crate::line_encoder::{LineEncodeError, LineEncoder};

pub use crate::fixed_length_decoder::{FixedLengthDecodeError, FixedLengthFrameDecoder};
pub use crate::length_field_decoder::{ByteOrder, LengthFieldBasedFrameDecoder, LengthFieldDecodeError};

pub use crate::length_field_prepender::{LengthFieldPrepender, LengthFieldPrependerError};
pub use crate::varint32::{ProtobufVarint32FrameDecoder, ProtobufVarint32LengthFieldPrepender, Varint32Error};
