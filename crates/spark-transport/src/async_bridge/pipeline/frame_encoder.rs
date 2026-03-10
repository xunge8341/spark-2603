use crate::async_bridge::OutboundFrame;

use crate::{KernelError, Result};

use crate::evidence::EvidenceSink;
use super::super::dyn_channel::DynChannel;

use super::context::ChannelHandlerContext;
use super::handler::ChannelHandler;
use super::super::channel_state::ChannelState;
use super::{DelimiterSpec, FrameDecoderProfile};

/// Built-in outbound framing encoder.
///
/// This handler provides **symmetric framing** for the built-in stream decoders:
/// - Line / Delimiter: appends the delimiter by default (avoids double-appending).
/// - LengthField / Varint32: prepends a length prefix.
/// - HTTP/1: pass-through (the management stack builds full responses).
///
/// Placement in the default pipeline (head -> tail):
/// `Head -> (extras first...) -> Encoder -> Decoder -> App -> (extras last...) -> Tail`.
///
/// Therefore on outbound (tail -> head), the encoder runs **before** the head handler,
/// right where it should be for production protocol stacks.
#[derive(Debug, Clone, Copy)]
pub struct StreamFrameEncoderHandler {
    profile: FrameDecoderProfile,
}

impl StreamFrameEncoderHandler {
    pub fn new(profile: FrameDecoderProfile) -> Self {
        Self { profile }
    }

    #[inline]
    fn encode_line(msg: OutboundFrame) -> OutboundFrame {
        // DECISION: Line framing must be idempotent and correct-by-default.
        if msg.ends_with(b"\n") {
            msg
        } else {
            msg.append_suffix(b"\n")
        }
    }

    #[inline]
    fn encode_delimiter(msg: OutboundFrame, delim: DelimiterSpec) -> OutboundFrame {
        let d = delim.as_slice();
        if d.is_empty() {
            return msg;
        }
        if msg.ends_with(d) {
            msg
        } else {
            msg.append_suffix(d)
        }
    }

    fn encode_length_field(
        msg: OutboundFrame,
        field_len: usize,
        order: spark_codec::ByteOrder,
    ) -> Result<OutboundFrame> {
        let prep = spark_codec::LengthFieldPrepender::try_new(field_len, order)
            .map_err(|_| KernelError::Invalid)?;

        let payload_len = msg.len();

        let mut hdr = [0u8; crate::async_bridge::OUTBOUND_INLINE_MAX];
        let n = prep
            .write_len(&mut hdr[..field_len], payload_len)
            .map_err(|_| KernelError::Invalid)?;

        // `write_len` returns the fixed field size; keep it generic anyway.
        msg.try_prepend_inline(&hdr[..n])
            .ok_or(KernelError::Invalid)
    }

    fn encode_varint32(msg: OutboundFrame) -> Result<OutboundFrame> {
        let payload_len = msg.len();
        if payload_len > (u32::MAX as usize) {
            return Err(KernelError::Invalid);
        }

        let mut hdr = [0u8; 5];
        let n = spark_codec::ProtobufVarint32LengthFieldPrepender::write_len(
            &mut hdr,
            payload_len as u32,
        );

        msg.try_prepend_inline(&hdr[..n])
            .ok_or(KernelError::Invalid)
    }
}

impl<A, Ev, Io> ChannelHandler<A, Ev, Io> for StreamFrameEncoderHandler
where
    Ev: EvidenceSink,
    Io: DynChannel,
{
    fn write(
        &mut self,
        ctx: &mut dyn ChannelHandlerContext<A>,
        _state: &mut ChannelState<Ev, Io>,
        msg: OutboundFrame,
    ) -> Result<()> {
        let out = match self.profile {
            FrameDecoderProfile::Line { .. } => Self::encode_line(msg),
            FrameDecoderProfile::Delimiter { delimiter, .. } => Self::encode_delimiter(msg, delimiter),
            FrameDecoderProfile::LengthField { field_len, order, .. } => {
                Self::encode_length_field(msg, field_len as usize, order)?
            }
            FrameDecoderProfile::Varint32 { .. } => Self::encode_varint32(msg)?,
            FrameDecoderProfile::Http1 { .. } => msg,
        };

        ctx.write(out)
    }
}
