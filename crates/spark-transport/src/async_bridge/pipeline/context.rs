use std::collections::VecDeque;

use spark_buffer::Bytes;

use crate::io::MsgBoundary;
use crate::{KernelError, Result};

use super::event::PipelineEvent;
use super::super::OutboundFrame;

/// Netty/DotNetty 风格的 `ChannelHandlerContext`（Rust best-practice 版本）。
///
/// 语义借鉴（保持清晰）：
/// - inbound：`fire_*` 让事件从 head -> tail 继续传播；
/// - outbound：`write/flush/close` 让事件从 tail -> head 继续传播。
///
/// Rust 实现方式（不照搬 Netty）：
/// - ctx **不是一个可长期保存的对象**；它只是一次 handler 调用期间的“事件写入器”；
/// - handler 调用 `ctx.fire_*` / `ctx.write` 只是把下一步封装成 `PipelineEvent` 推入队列；
/// - pipeline 用 `pump()` 循环取出事件并执行（trampoline），避免递归/重入与 unsafe。
///
/// 为什么不用 `Rc<RefCell<..>>`：
/// - 直接持有 `&mut VecDeque`，避免 RefCell 运行期借用错误与额外开销；
/// - 通过 trait object（`&mut dyn ChannelHandlerContext<A>`）保持 handler object-safe。
pub trait ChannelHandlerContext<A> {
    // ---------------- inbound ----------------
    fn fire_channel_active(&mut self) -> Result<()>;
    fn fire_channel_inactive(&mut self) -> Result<()>;
    fn fire_channel_read_raw(&mut self, boundary: MsgBoundary, bytes: Bytes) -> Result<()>;
    fn fire_channel_read(&mut self, msg: Bytes) -> Result<()>;
    fn fire_channel_read_complete(&mut self) -> Result<()>;
    fn fire_exception_caught(&mut self, err: KernelError) -> Result<()>;
    fn fire_channel_writability_changed(&mut self, is_writable: bool) -> Result<()>;
    fn fire_app_complete(&mut self, result: core::result::Result<Option<Bytes>, KernelError>) -> Result<()>;

    // ---------------- outbound ----------------
    fn write(&mut self, msg: OutboundFrame) -> Result<()>;
    fn flush(&mut self) -> Result<()>;
    fn close(&mut self) -> Result<()>;
}

/// pipeline 内部使用的 ctx 实现。
///
/// `index` 表示“当前正在执行的 handler 的索引”。
/// - inbound 的 next = index + 1
/// - outbound 的 prev = index - 1
pub(crate) struct EventCtx<'q, A> {
    pub(crate) queue: &'q mut VecDeque<PipelineEvent>,
    pub(crate) len: usize,
    pub(crate) index: usize,
    // 仅用于把 A 绑定进类型系统，避免误用。
    phantom: core::marker::PhantomData<fn(A) -> A>,
}

impl<'q, A> EventCtx<'q, A> {
    #[inline]
    pub(crate) fn new(queue: &'q mut VecDeque<PipelineEvent>, len: usize, index: usize) -> Self {
        Self {
            queue,
            len,
            index,
            phantom: core::marker::PhantomData,
        }
    }

    #[inline]
    fn enqueue(&mut self, ev: PipelineEvent) {
        self.queue.push_back(ev);
    }

    #[inline]
    fn next_inbound(&self) -> Option<usize> {
        let next = self.index + 1;
        if next >= self.len {
            None
        } else {
            Some(next)
        }
    }

    #[inline]
    fn prev_outbound(&self) -> Option<usize> {
        self.index.checked_sub(1)
    }
}

impl<'q, A> ChannelHandlerContext<A> for EventCtx<'q, A> {
    // ---------------- inbound：向后传播（index+1） ----------------
    #[inline]
    fn fire_channel_active(&mut self) -> Result<()> {
        if let Some(start) = self.next_inbound() {
            self.enqueue(PipelineEvent::ChannelActive { start });
        }
        Ok(())
    }

    #[inline]
    fn fire_channel_inactive(&mut self) -> Result<()> {
        if let Some(start) = self.next_inbound() {
            self.enqueue(PipelineEvent::ChannelInactive { start });
        }
        Ok(())
    }

    #[inline]
    fn fire_channel_read_raw(&mut self, boundary: MsgBoundary, bytes: Bytes) -> Result<()> {
        if let Some(start) = self.next_inbound() {
            self.enqueue(PipelineEvent::ChannelReadRaw { start, boundary, bytes });
        }
        Ok(())
    }

    #[inline]
    fn fire_channel_read(&mut self, msg: Bytes) -> Result<()> {
        if let Some(start) = self.next_inbound() {
            self.enqueue(PipelineEvent::ChannelRead { start, msg });
        }
        Ok(())
    }

    #[inline]
    fn fire_channel_read_complete(&mut self) -> Result<()> {
        if let Some(start) = self.next_inbound() {
            self.enqueue(PipelineEvent::ChannelReadComplete { start });
        }
        Ok(())
    }

    #[inline]
    fn fire_exception_caught(&mut self, err: KernelError) -> Result<()> {
        if let Some(start) = self.next_inbound() {
            self.enqueue(PipelineEvent::ExceptionCaught { start, err });
        }
        Ok(())
    }

    #[inline]
    fn fire_channel_writability_changed(&mut self, is_writable: bool) -> Result<()> {
        if let Some(start) = self.next_inbound() {
            self.enqueue(PipelineEvent::WritabilityChanged { start, is_writable });
        }
        Ok(())
    }

    #[inline]
    fn fire_app_complete(&mut self, result: core::result::Result<Option<Bytes>, KernelError>) -> Result<()> {
        if let Some(start) = self.next_inbound() {
            self.enqueue(PipelineEvent::AppComplete { start, result });
        }
        Ok(())
    }

    // ---------------- outbound：向前传播（index-1） ----------------
    #[inline]
    fn write(&mut self, msg: OutboundFrame) -> Result<()> {
        if let Some(start) = self.prev_outbound() {
            self.enqueue(PipelineEvent::Write { start, msg });
        }
        Ok(())
    }

    #[inline]
    fn flush(&mut self) -> Result<()> {
        if let Some(start) = self.prev_outbound() {
            self.enqueue(PipelineEvent::Flush { start });
        }
        Ok(())
    }

    #[inline]
    fn close(&mut self) -> Result<()> {
        if let Some(start) = self.prev_outbound() {
            self.enqueue(PipelineEvent::Close { start });
        }
        Ok(())
    }
}
