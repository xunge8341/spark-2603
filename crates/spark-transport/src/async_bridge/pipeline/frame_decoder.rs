use spark_core::context::Context as DecodeContext;
use crate::io::MsgBoundary;
use crate::{KernelError, Result};
use crate::evidence::EvidenceSink;
use super::super::dyn_channel::DynChannel;

use spark_buffer::Bytes;

#[cfg(feature = "mgmt-http1")]
use super::http1_inbound::{Http1AppendError, Http1InboundState};

use super::context::ChannelHandlerContext;
use super::handler::ChannelHandler;
use super::super::channel_state::ChannelState;
use super::super::inbound_state::InboundState;
use super::FrameDecoderProfile;

/// TCP stream 的 framing handler（对齐 Netty `ByteToMessageDecoder` 的职责位置）。
///
/// - 对于 `MsgBoundary::None`（stream）：维护 cumulation，并运行 decoder loop。
/// - 对于 `MsgBoundary::Complete`（datagram）：直接作为一条消息向后传播。
///
/// bring-up 默认协议：line-based（见 `InboundState`）。未来可替换为
/// length-field / WebSocket / 自定义协议。
#[derive(Debug)]
pub struct StreamFrameDecoderHandler {
    profile: FrameDecoderProfile,
    inbound: InboundState,
    decode_ctx: DecodeContext,

    #[cfg(feature = "mgmt-http1")]
    http1: Option<Http1InboundState>,
}

impl StreamFrameDecoderHandler {
    pub fn new(profile: FrameDecoderProfile) -> Self {
        let inbound = match profile {
            FrameDecoderProfile::Line { max_frame } => InboundState::new_line(max_frame),
            FrameDecoderProfile::Delimiter { max_frame, delimiter, include_delimiter } => {
                InboundState::new_delimiter(max_frame, delimiter, include_delimiter)
            }
            FrameDecoderProfile::LengthField { max_frame, field_len, order } => {
                InboundState::new_length_field(max_frame, field_len as usize, order)
                    .unwrap_or_else(|| InboundState::new_line(max_frame))
            }
            FrameDecoderProfile::Varint32 { max_frame } => InboundState::new_varint32(max_frame),
            FrameDecoderProfile::Http1 { max_request_bytes, .. } => {
                // Unused for HTTP/1 profile (handled by `http1` state), but keep it initialized.
                InboundState::new_line(max_request_bytes)
            }
        };

        Self {
            profile,
            inbound,
            decode_ctx: DecodeContext::default(),

            #[cfg(feature = "mgmt-http1")]
            http1: match profile {
                FrameDecoderProfile::Http1 { max_request_bytes, max_head_bytes, max_headers } => {
                    Some(Http1InboundState::new(max_request_bytes, max_head_bytes, max_headers))
                }
                _ => None,
            },
        }
    }

    pub(crate) fn on_stream_bytes<A, Ev, Io>(
        &mut self,
        ctx: &mut dyn ChannelHandlerContext<A>,
        state: &mut ChannelState<Ev, Io>,
        bytes: &[u8],
    ) -> Result<()>
    where
        Ev: EvidenceSink,
        Io: DynChannel,
    {
        match self.profile {
            FrameDecoderProfile::Line { .. }
            | FrameDecoderProfile::Delimiter { .. }
            | FrameDecoderProfile::LengthField { .. }
            | FrameDecoderProfile::Varint32 { .. } => {
                match self.inbound.append_stream_bytes(&mut self.decode_ctx, bytes) {
                    Ok(batch) => {
                        if batch.coalesce_count > 0 || batch.copied_bytes > 0 {
                            state.record_inbound_cumulation(batch.coalesce_count, batch.copied_bytes);
                        }
                        while let Some(msg) = self.inbound.pop_ready() {
                            state.record_decoded(1);
                            ctx.fire_channel_read(msg)?;
                        }
                        Ok(())
                    }
                    Err(e) => {
                        state.record_decode_error();
                        // If the cumulation already exceeded the configured limit, surface it as a dedicated signal.
                        let size = self.inbound.cumulation_len();
                        let max = self.profile.max_frame_hint();
                        if size > max {
                            state.record_frame_too_large(size, max);
                        }
                        ctx.fire_exception_caught(e)
                    }
                }
            }
            FrameDecoderProfile::Http1 { .. } => {
                #[cfg(feature = "mgmt-http1")]
                {
                    let Some(http1) = self.http1.as_mut() else {
                        return Err(KernelError::Internal(crate::error_codes::ERR_INTERNAL_HTTP1_STATE));
                    };

                    match http1.append_stream_bytes(&mut self.decode_ctx, bytes) {
                        Ok(batch) => {
                            if batch.coalesce_count > 0 || batch.copied_bytes > 0 {
                                state.record_inbound_cumulation(batch.coalesce_count, batch.copied_bytes);
                            }
                            while let Some(req) = http1.pop_ready() {
                                state.record_decoded(1);
                                ctx.fire_channel_read(req)?;
                            }
                            Ok(())
                        }
                        Err(Http1AppendError::TooLarge { size, max }) => {
                            state.record_decode_error();
                            state.record_frame_too_large(size, max);
                            ctx.fire_exception_caught(KernelError::Invalid)
                        }
                        Err(Http1AppendError::Decode) => {
                            state.record_decode_error();
                            ctx.fire_exception_caught(KernelError::Invalid)
                        }
                    }
                }

                #[cfg(not(feature = "mgmt-http1"))]
                {
                    let _ = bytes;
                    let _ = ctx;
                    let _ = state;
                    Err(KernelError::Invalid)
                }
            }
        }
    }
}

impl<A, Ev, Io> ChannelHandler<A, Ev, Io> for StreamFrameDecoderHandler
where
    Ev: EvidenceSink,
    Io: DynChannel,
{

    fn channel_read_raw(
        &mut self,
        ctx: &mut dyn ChannelHandlerContext<A>,
        state: &mut ChannelState<Ev, Io>,
        boundary: MsgBoundary,
        bytes: Bytes,
    ) -> Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }

        match boundary {
            MsgBoundary::Complete => {
                // Datagram：天然消息边界，直接向后传播。
                state.record_decoded(1);
                ctx.fire_channel_read(bytes)
            }
            MsgBoundary::None => {
                // Stream：cumulation + decode loop。
                self.on_stream_bytes(ctx, state, bytes.as_ref())
            }
        }
    }

    fn channel_inactive(
        &mut self,
        ctx: &mut dyn ChannelHandlerContext<A>,
        _state: &mut ChannelState<Ev, Io>,
    ) -> Result<()> {
        // 清理 cumulation 与队列（释放内存/避免复用污染）。
        self.inbound = match self.profile {
            FrameDecoderProfile::Line { max_frame } => InboundState::new_line(max_frame),
            FrameDecoderProfile::Delimiter { max_frame, delimiter, include_delimiter } => {
                InboundState::new_delimiter(max_frame, delimiter, include_delimiter)
            }
            FrameDecoderProfile::LengthField { max_frame, field_len, order } => {
                InboundState::new_length_field(max_frame, field_len as usize, order)
                    .unwrap_or_else(|| InboundState::new_line(max_frame))
            }
            FrameDecoderProfile::Varint32 { max_frame } => InboundState::new_varint32(max_frame),
            FrameDecoderProfile::Http1 { max_request_bytes, .. } => InboundState::new_line(max_request_bytes),
        };

        #[cfg(feature = "mgmt-http1")]
        {
            self.http1 = match self.profile {
                FrameDecoderProfile::Http1 { max_request_bytes, max_head_bytes, max_headers } => {
                    Some(Http1InboundState::new(max_request_bytes, max_head_bytes, max_headers))
                }
                _ => None,
            };
        }
        ctx.fire_channel_inactive()
    }

    fn exception_caught(
        &mut self,
        ctx: &mut dyn ChannelHandlerContext<A>,
        _state: &mut ChannelState<Ev, Io>,
        err: KernelError,
    ) -> Result<()> {
        ctx.fire_exception_caught(err)
    }
}
