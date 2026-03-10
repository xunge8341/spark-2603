use crate::async_bridge::OutboundFrame;

use crate::{KernelError, Result};

use crate::evidence::EvidenceSink;
use super::super::dyn_channel::DynChannel;

use super::context::ChannelHandlerContext;
use super::handler::ChannelHandler;
use super::super::channel_state::ChannelState;

/// pipeline 的 head handler（Netty/DotNetty：HeadContext）。
///
/// 职责：
/// - outbound 方向的最终落点：把 bytes 入队到 `OutboundBuffer`。
/// - I/O drain（真正写出）由 driver 在 Writable/flush 时机统一执行，以便集中做 metrics/interest。
#[derive(Debug, Default)]
pub struct HeadHandler;

impl<A, Ev, Io> ChannelHandler<A, Ev, Io> for HeadHandler
where
    Ev: EvidenceSink,
    Io: DynChannel,
{

    fn channel_inactive(
        &mut self,
        ctx: &mut dyn ChannelHandlerContext<A>,
        state: &mut ChannelState<Ev, Io>,
    ) -> Result<()> {
        // Emit CloseComplete evidence from the semantic layer (stable across backends).
        state.emit_close_complete();
        ctx.fire_channel_inactive()
    }


    fn write(
        &mut self,
        ctx: &mut dyn ChannelHandlerContext<A>,
        state: &mut ChannelState<Ev, Io>,
        msg: OutboundFrame,
    ) -> Result<()> {
        // 语义：enqueue（不在这里做 syscalls）。
        state.enqueue_outbound(msg);
        // outbound 继续向前传播到更“底层”的 handler 已无意义，因此直接返回。
        // 如果未来引入真正的 TransportWriter（负责 writev/drain），可以在此处继续向前。
        let _ = ctx; // 保持接口一致（未来扩展）。
        Ok(())
    }

    fn flush(
        &mut self,
        ctx: &mut dyn ChannelHandlerContext<A>,
        _state: &mut ChannelState<Ev, Io>,
    ) -> Result<()> {
        // bring-up 版本：flush 事件仅作为“语义边界”。
        // - encoder/聚合器可以在 flush 时把内部缓冲写入 outbound。
        // - 真实 I/O drain 由 driver 统一做（Writable 或 app-complete 之后）。
        let _ = ctx;
        Ok(())
    }

    fn close(
        &mut self,
        ctx: &mut dyn ChannelHandlerContext<A>,
        state: &mut ChannelState<Ev, Io>,
    ) -> Result<()> {
        // Netty/DotNetty：HeadContext 最终负责把 close 语义落实到底层 IO。
        // bring-up：直接请求关闭（底层 io.close() 若不支持会返回 Unsupported，但语义上仍视为 close 已请求）。
        state.request_close();
        let _ = ctx;
        Ok(())
    }

    fn exception_caught(
        &mut self,
        ctx: &mut dyn ChannelHandlerContext<A>,
        _state: &mut ChannelState<Ev, Io>,
        err: KernelError,
    ) -> Result<()> {
        // 默认透传。
        ctx.fire_exception_caught(err)
    }
}
