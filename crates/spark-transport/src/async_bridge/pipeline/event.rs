use spark_buffer::Bytes;

use super::super::OutboundFrame;

use crate::io::MsgBoundary;
use crate::KernelError;

/// Pipeline 的“事件队列”元素（trampoline 模型）。
///
/// 设计要点（Rust 最佳实践 + Netty 语义借鉴）：
/// - handler 之间的传播不再使用递归/指针跳转，而是 **入队事件**；
/// - pipeline 以循环（pump）方式逐个出队并调用 handler；
/// - handler 内部调用 `ctx.fire_* / ctx.write / ctx.flush` 只是继续 **入队**，避免重入。
///
/// 这样可以：
/// - 显著减少（甚至消除）`unsafe`；
/// - 避免深递归导致的栈增长；
/// - 语义仍保持 Netty/DotNetty 的 inbound/outbound 双向传播。
#[derive(Debug)]
pub(crate) enum PipelineEvent {
    // ---------------- inbound（head -> tail） ----------------
    ChannelActive { start: usize },
    ChannelInactive { start: usize },
    ChannelReadRaw {
        start: usize,
        boundary: MsgBoundary,
        bytes: Bytes,
    },
    ChannelRead { start: usize, msg: Bytes },
    ChannelReadComplete { start: usize },
    ExceptionCaught { start: usize, err: KernelError },
    WritabilityChanged { start: usize, is_writable: bool },
    AppComplete {
        start: usize,
        result: core::result::Result<Option<Bytes>, KernelError>,
    },

    // ---------------- outbound（tail -> head） ----------------
    Write { start: usize, msg: OutboundFrame },
    Flush { start: usize },
    Close { start: usize },
}
