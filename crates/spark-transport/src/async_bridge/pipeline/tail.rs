use spark_buffer::Bytes;

use crate::{KernelError, Result};
use crate::evidence::EvidenceSink;
use crate::io::MsgBoundary;
use super::super::dyn_channel::DynChannel;

use super::context::ChannelHandlerContext;
use super::handler::ChannelHandler;
use super::super::channel_state::ChannelState;

/// pipeline 的 tail handler（Netty/DotNetty：TailContext）。
///
/// 职责：
/// - 兜底处理：当 inbound 消息没有被业务 handler 消费时，直接丢弃。
/// - 兜底异常策略：默认触发 close（bring-up 阶段简化处理）。
#[derive(Debug, Default)]
pub struct TailHandler;

impl<A, Ev, Io> ChannelHandler<A, Ev, Io> for TailHandler
where
    Ev: EvidenceSink,
    Io: DynChannel,
{

    fn channel_read_raw(
        &mut self,
        _ctx: &mut dyn ChannelHandlerContext<A>,
        _state: &mut ChannelState<Ev, Io>,
        _boundary: MsgBoundary,
        _bytes: Bytes,
    ) -> Result<()> {
        // 未被 decoder/handler 处理的原始字节，丢弃。
        Ok(())
    }

    fn channel_read(
        &mut self,
        _ctx: &mut dyn ChannelHandlerContext<A>,
        _state: &mut ChannelState<Ev, Io>,
        _msg: Bytes,
    ) -> Result<()> {
        // 未被业务 handler 消费的消息，丢弃。
        Ok(())
    }

    fn exception_caught(
        &mut self,
        ctx: &mut dyn ChannelHandlerContext<A>,
        _state: &mut ChannelState<Ev, Io>,
        _err: KernelError,
    ) -> Result<()> {
        // bring-up 阶段：异常兜底关闭。
        ctx.close()
    }

    fn on_app_complete(
        &mut self,
        _ctx: &mut dyn ChannelHandlerContext<A>,
        _state: &mut ChannelState<Ev, Io>,
        _result: core::result::Result<Option<Bytes>, KernelError>,
    ) -> Result<()> {
        // app complete 事件如果走到 tail，说明没有 service handler。
        Ok(())
    }
}
