use spark_buffer::Bytes;

use crate::io::MsgBoundary;
use crate::{KernelError, Result};

use crate::evidence::EvidenceSink;
use super::super::dyn_channel::DynChannel;

use super::context::ChannelHandlerContext;
use super::super::channel_state::ChannelState;
use super::super::task::AppFuture;
use super::super::OutboundFrame;

/// Netty/DotNetty 风格的 handler。
///
/// 约定：
/// - inbound 事件从 head -> tail 方向传播（索引递增）。
/// - outbound 事件从 tail -> head 方向传播（索引递减）。
/// - 默认实现均为“透传”，保持语义清晰。
///
/// Rust 最佳实践权衡：
/// - handler API 保持同步（不返回 Future），避免 future 类型泄露。
/// - 异步业务由 `AppServiceHandler` 内部创建 `AppFuture`，driver 统一 poll。
///
/// 注意：handler **不要求 Send**。
///
/// 原因（DotNetty/Netty 经验 + Rust 权衡）：
/// - 在 Netty 模型里，同一个 Channel 的 pipeline 通常被固定在同一个 EventLoop 线程驱动；
/// - 业务 `Service` 返回的 Future 可能不是 `Send`（例如仅在当前线程 poll）；
/// - 因此把 `Send` 作为硬约束会造成不必要的限制。
///
/// 约束靠不变式保证：pipeline/handler 只能在所属 driver 线程内访问。
pub trait ChannelHandler<A, Ev, Io = Box<dyn DynChannel>>
where
    Ev: EvidenceSink,
    Io: DynChannel,
{

    /// 连接激活（通常在 install/register 后触发）。
    fn channel_active(
        &mut self,
        ctx: &mut dyn ChannelHandlerContext<A>,
        _state: &mut ChannelState<Ev, Io>,
    ) -> Result<()> {
        ctx.fire_channel_active()
    }

    /// 连接关闭/失活。
    fn channel_inactive(
        &mut self,
        ctx: &mut dyn ChannelHandlerContext<A>,
        _state: &mut ChannelState<Ev, Io>,
    ) -> Result<()> {
        ctx.fire_channel_inactive()
    }

    /// 低层 read 读到的“原始字节片段”。
    ///
    /// - `boundary=Complete` 表示 datagram（天然消息边界）。
    /// - `boundary=None` 表示 stream（TCP），必须走 cumulation + decoder。
    fn channel_read_raw(
        &mut self,
        ctx: &mut dyn ChannelHandlerContext<A>,
        _state: &mut ChannelState<Ev, Io>,
        boundary: MsgBoundary,
        bytes: Bytes,
    ) -> Result<()> {
        ctx.fire_channel_read_raw(boundary, bytes)
    }

    /// 已经 framing 后的“消息”。业务 handler 只应看到这一层。
    fn channel_read(
        &mut self,
        ctx: &mut dyn ChannelHandlerContext<A>,
        _state: &mut ChannelState<Ev, Io>,
        msg: Bytes,
    ) -> Result<()> {
        ctx.fire_channel_read(msg)
    }

    /// 一次 readable loop 完成（read spin结束）。
    fn channel_read_complete(
        &mut self,
        ctx: &mut dyn ChannelHandlerContext<A>,
        _state: &mut ChannelState<Ev, Io>,
    ) -> Result<()> {
        ctx.fire_channel_read_complete()
    }

    /// 异常处理（codec/IO/app）。
    fn exception_caught(
        &mut self,
        ctx: &mut dyn ChannelHandlerContext<A>,
        _state: &mut ChannelState<Ev, Io>,
        err: KernelError,
    ) -> Result<()> {
        ctx.fire_exception_caught(err)
    }

    /// 可写性变化（水位线驱动）。
    fn channel_writability_changed(
        &mut self,
        ctx: &mut dyn ChannelHandlerContext<A>,
        _state: &mut ChannelState<Ev, Io>,
        is_writable: bool,
    ) -> Result<()> {
        ctx.fire_channel_writability_changed(is_writable)
    }

    // ---------------- outbound ----------------

    /// 写入一段 bytes（语义：enqueue）。
    fn write(
        &mut self,
        ctx: &mut dyn ChannelHandlerContext<A>,
        _state: &mut ChannelState<Ev, Io>,
        msg: OutboundFrame,
    ) -> Result<()> {
        ctx.write(msg)
    }

    /// flush 事件（语义：促使 encoder/聚合器把内部缓冲写入 outbound）。
    fn flush(
        &mut self,
        ctx: &mut dyn ChannelHandlerContext<A>,
        _state: &mut ChannelState<Ev, Io>,
    ) -> Result<()> {
        ctx.flush()
    }

    /// 关闭连接。
    fn close(
        &mut self,
        ctx: &mut dyn ChannelHandlerContext<A>,
        _state: &mut ChannelState<Ev, Io>,
    ) -> Result<()> {
        ctx.close()
    }

    // --------- 业务 Future 管理（仅 service handler 实现） ---------

    /// driver 从 pipeline 取走“待 poll 的业务 future”。
    fn take_app_future(&mut self) -> Option<AppFuture> {
        None
    }

    /// 是否有排队的业务请求（用于 driver 的快速继续策略）。
    fn has_app_backlog(&self) -> bool {
        false
    }

    /// driver 在业务 future 完成后回调 pipeline（service handler 在此写回响应）。
    fn on_app_complete(
        &mut self,
        ctx: &mut dyn ChannelHandlerContext<A>,
        _state: &mut ChannelState<Ev, Io>,
        result: core::result::Result<Option<Bytes>, KernelError>,
    ) -> Result<()> {
        ctx.fire_app_complete(result)
    }
}
