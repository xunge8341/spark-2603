#![no_std]

mod byte_to_message;
mod delimiter_decoder;
mod delimiter_encoder;
mod line_decoder;
mod line_encoder;
mod fixed_length_decoder;
mod length_field_decoder;
mod length_field_prepender;
mod varint32;
mod decoder;
mod encoder;
mod intent;
pub mod delimiters;
pub mod serial;

pub mod prelude;

pub use byte_to_message::{ByteToMessageDecoder, DecodeOutcome};
pub use delimiter_decoder::{DelimiterBasedFrameDecoder, DelimiterDecodeError};
pub use delimiter_encoder::{DelimiterEncodeError, DelimiterEncoder};
pub use line_decoder::{LineDecodeError, LineDecoder};
pub use line_encoder::{LineEncodeError, LineEncoder};
pub use fixed_length_decoder::{FixedLengthDecodeError, FixedLengthFrameDecoder};
pub use length_field_decoder::{ByteOrder, LengthFieldBasedFrameDecoder, LengthFieldDecodeError};
pub use length_field_prepender::{LengthFieldPrepender, LengthFieldPrependerError};
pub use varint32::{ProtobufVarint32FrameDecoder, ProtobufVarint32LengthFieldPrepender, Varint32Error};
pub use serial::{CobsFrameDecoder, HdlcFrameDecoder, SlipFrameDecoder};
pub use decoder::Decoder;
pub use encoder::Encoder;
pub use intent::Intent;
