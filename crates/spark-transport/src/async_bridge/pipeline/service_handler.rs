use std::collections::VecDeque;
use std::sync::Arc;

use spark_buffer::Bytes;

use crate::evidence::EvidenceSink;
use crate::{KernelError, Result};
use super::super::dyn_channel::DynChannel;
use spark_core::context::Context as BizContext;
use spark_core::service::Service;

use super::context::ChannelHandlerContext;
use super::handler::ChannelHandler;
use super::super::channel_state::ChannelState;
use super::super::task::AppFuture;
use super::super::OutboundFrame;

/// 业务 Service handler（DotNetty 经验：pipeline 同步、业务异步通过 Task/Future 承载）。
///
/// 行为：
/// - 收到一条消息（已 framing）后，如果当前无 inflight，则创建 `AppFuture`。
/// - 如果已有 inflight，则进入队列（严格 FIFO）。
/// - driver poll 完成后，调用 `on_app_complete`：写回响应并启动下一条。
// 注意：这里不 derive(Debug)，因为 `AppFuture`（dyn Future）不实现 Debug。
pub struct AppServiceHandler<A> {
    app: Arc<A>,

    // 仅允许单 inflight（bring-up 阶段简化）。
    inflight: Option<AppFuture>,

    // 后续请求排队。
    queue: VecDeque<Bytes>,
}

impl<A> AppServiceHandler<A>
where
    A: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
{
    pub fn new(app: Arc<A>) -> Self {
        Self {
            app,
            inflight: None,
            queue: VecDeque::new(),
        }
    }

    fn start_call(&self, req: Bytes) -> AppFuture {
        let app = Arc::clone(&self.app);
        let ctx = BizContext::default();
        Box::pin(async move { app.call(ctx, req).await })
    }

    fn maybe_start_next(&mut self) {
        if self.inflight.is_some() {
            return;
        }
        if let Some(req) = self.queue.pop_front() {
            self.inflight = Some(self.start_call(req));
        }
    }

    /// Driver hook: take the currently in-flight application future (if any).
    ///
    /// This is intentionally an **inherent** method (not only a trait method):
    /// it avoids type-inference ambiguity when `Ev/Io` are generic parameters
    /// elsewhere in the pipeline.
    #[inline]
    pub fn take_app_future(&mut self) -> Option<AppFuture> {
        self.inflight.take()
    }

    /// Driver hook: whether the application handler has queued requests.
    ///
    /// Inherent for the same reason as `take_app_future`.
    #[inline]
    pub fn has_app_backlog(&self) -> bool {
        !self.queue.is_empty()
    }
}

impl<A, Ev, Io> ChannelHandler<A, Ev, Io> for AppServiceHandler<A>
where
    A: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
    Ev: EvidenceSink,
    Io: DynChannel,
{
    fn channel_read(
        &mut self,
        _ctx: &mut dyn ChannelHandlerContext<A>,
        _state: &mut ChannelState<Ev, Io>,
        msg: Bytes,
    ) -> Result<()> {
        // Netty/DotNetty 的典型规则：保持 handler 非阻塞。
        if self.inflight.is_none() {
            self.inflight = Some(self.start_call(msg));
        } else {
            // 简单 FIFO 排队。
            self.queue.push_back(msg);
        }

        // 继续向后传播（若还有其他业务 handler）。bring-up 默认 tail 会丢弃。
        Ok(())
    }

    fn take_app_future(&mut self) -> Option<AppFuture> {
        self.inflight.take()
    }

    fn has_app_backlog(&self) -> bool {
        !self.queue.is_empty()
    }

    fn on_app_complete(
        &mut self,
        ctx: &mut dyn ChannelHandlerContext<A>,
        _state: &mut ChannelState<Ev, Io>,
        result: core::result::Result<Option<Bytes>, KernelError>,
    ) -> Result<()> {
        match result {
            Ok(Some(resp)) => {
                // outbound：向前传播（tail->head）。
                ctx.write(OutboundFrame::from_bytes(resp))?;
                ctx.flush()?;
            }
            Ok(None) => {}
            Err(e) => {
                // 业务异常：交给 pipeline exception 机制。
                ctx.fire_exception_caught(e)?;
            }
        }

        // 启动下一条请求（如果有）。
        self.maybe_start_next();

        // 继续向后传播（如果 pipeline 里还有其他 handler）。
        Ok(())
    }

    fn channel_inactive(
        &mut self,
        ctx: &mut dyn ChannelHandlerContext<A>,
        _state: &mut ChannelState<Ev, Io>,
    ) -> Result<()> {
        self.inflight = None;
        self.queue.clear();
        ctx.fire_channel_inactive()
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
