use std::collections::VecDeque;
use std::sync::Arc;

use spark_buffer::Bytes;

use super::super::dyn_channel::DynChannel;
use crate::evidence::EvidenceSink;
use crate::{KernelError, Result};
use spark_core::context::Context as BizContext;
use spark_core::service::Service;

use super::super::channel_state::ChannelState;
use super::super::task::AppFuture;
use super::super::OutboundFrame;
use super::context::ChannelHandlerContext;
use super::handler::ChannelHandler;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverloadAction {
    FailFast,
    Backpressure,
    CloseConnection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppServiceOptions {
    pub max_inflight_per_connection: usize,
    pub max_queue_per_connection: usize,
    pub overload_action: OverloadAction,
}

impl AppServiceOptions {
    #[inline]
    pub fn normalized(mut self) -> Self {
        self.max_inflight_per_connection = self.max_inflight_per_connection.max(1);
        self
    }
}

impl Default for AppServiceOptions {
    fn default() -> Self {
        Self {
            max_inflight_per_connection: 1,
            max_queue_per_connection: 1024,
            overload_action: OverloadAction::FailFast,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct AppOverloadStats {
    pub reject_total: u64,
    pub backpressure_total: u64,
    pub close_total: u64,
    pub queue_high_watermark: u64,
}

/// 业务 Service handler（DotNetty 经验：pipeline 同步、业务异步通过 Task/Future 承载）。
///
/// 行为：
/// - 收到一条消息（已 framing）后，如果当前无 inflight，则创建 `AppFuture`。
/// - 如果已有 inflight，则进入队列（严格 FIFO）。
/// - driver poll 完成后，调用 `on_app_complete`：写回响应并启动下一条。
// 注意：这里不 derive(Debug)，因为 `AppFuture`（dyn Future）不实现 Debug。
pub struct AppServiceHandler<A> {
    app: Arc<A>,
    opts: AppServiceOptions,

    // driver may poll one future at a time; this queue holds ready-to-poll futures.
    inflight_futures: VecDeque<AppFuture>,

    // started and not yet completed.
    active_calls: usize,

    // 后续请求排队。
    queue: VecDeque<Bytes>,

    overload_reject_total: u64,
    overload_backpressure_total: u64,
    overload_close_total: u64,
    queue_high_watermark: u64,
}

impl<A> AppServiceHandler<A>
where
    A: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
{
    pub fn new(app: Arc<A>, opts: AppServiceOptions) -> Self {
        let opts = opts.normalized();
        Self {
            app,
            opts,
            inflight_futures: VecDeque::new(),
            active_calls: 0,
            queue: VecDeque::new(),
            overload_reject_total: 0,
            overload_backpressure_total: 0,
            overload_close_total: 0,
            queue_high_watermark: 0,
        }
    }

    fn start_call(&self, req: Bytes) -> AppFuture {
        let app = Arc::clone(&self.app);
        let ctx = BizContext::default();
        Box::pin(async move { app.call(ctx, req).await })
    }

    fn maybe_start_next(&mut self) {
        while self.active_calls < self.opts.max_inflight_per_connection {
            let Some(req) = self.queue.pop_front() else {
                break;
            };
            self.inflight_futures.push_back(self.start_call(req));
            self.active_calls += 1;
        }
    }

    /// Driver hook: take the currently in-flight application future (if any).
    ///
    /// This is intentionally an **inherent** method (not only a trait method):
    /// it avoids type-inference ambiguity when `Ev/Io` are generic parameters
    /// elsewhere in the pipeline.
    #[inline]
    pub fn take_app_future(&mut self) -> Option<AppFuture> {
        self.inflight_futures.pop_front()
    }

    /// Driver hook: whether the application handler has queued requests.
    ///
    /// Inherent for the same reason as `take_app_future`.
    #[inline]
    pub fn has_app_backlog(&self) -> bool {
        !self.queue.is_empty() || !self.inflight_futures.is_empty()
    }

    #[inline]
    fn note_queue_high_watermark(&mut self) {
        let q = self.queue.len() as u64;
        if q > self.queue_high_watermark {
            self.queue_high_watermark = q;
        }
    }

    #[inline]
    pub fn take_overload_stats(&mut self) -> AppOverloadStats {
        let stats = AppOverloadStats {
            reject_total: self.overload_reject_total,
            backpressure_total: self.overload_backpressure_total,
            close_total: self.overload_close_total,
            queue_high_watermark: self.queue_high_watermark,
        };
        self.overload_reject_total = 0;
        self.overload_backpressure_total = 0;
        self.overload_close_total = 0;
        stats
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
        ctx: &mut dyn ChannelHandlerContext<A>,
        _state: &mut ChannelState<Ev, Io>,
        msg: Bytes,
    ) -> Result<()> {
        // Netty/DotNetty 的典型规则：保持 handler 非阻塞。
        if self.active_calls < self.opts.max_inflight_per_connection {
            self.inflight_futures.push_back(self.start_call(msg));
            self.active_calls += 1;
        } else if self.queue.len() < self.opts.max_queue_per_connection {
            self.queue.push_back(msg);
            self.note_queue_high_watermark();
        } else {
            match self.opts.overload_action {
                OverloadAction::FailFast => {
                    self.overload_reject_total = self.overload_reject_total.saturating_add(1);
                    ctx.fire_exception_caught(KernelError::NoMem)?;
                }
                OverloadAction::Backpressure => {
                    self.overload_backpressure_total =
                        self.overload_backpressure_total.saturating_add(1);
                    ctx.fire_channel_writability_changed(false)?;
                }
                OverloadAction::CloseConnection => {
                    self.overload_close_total = self.overload_close_total.saturating_add(1);
                    ctx.close()?;
                }
            }
        }

        // 继续向后传播（若还有其他业务 handler）。bring-up 默认 tail 会丢弃。
        Ok(())
    }

    fn take_app_future(&mut self) -> Option<AppFuture> {
        self.inflight_futures.pop_front()
    }

    fn has_app_backlog(&self) -> bool {
        !self.queue.is_empty() || !self.inflight_futures.is_empty()
    }

    fn on_app_complete(
        &mut self,
        ctx: &mut dyn ChannelHandlerContext<A>,
        _state: &mut ChannelState<Ev, Io>,
        result: core::result::Result<Option<Bytes>, KernelError>,
    ) -> Result<()> {
        if self.active_calls > 0 {
            self.active_calls -= 1;
        }

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
        self.inflight_futures.clear();
        self.active_calls = 0;
        self.queue.clear();
        self.overload_reject_total = 0;
        self.overload_backpressure_total = 0;
        self.overload_close_total = 0;
        self.queue_high_watermark = 0;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::async_bridge::channel_state::ChannelState;
    use crate::async_bridge::dyn_channel::DynChannel;
    use crate::async_bridge::pipeline::context::ChannelHandlerContext;
    use crate::evidence::NoopEvidenceSink;
    use crate::io::{caps, ChannelCaps, IoOps, ReadOutcome, RxToken};
    use crate::policy::FlushPolicy;
    use spark_core::context::Context as BizContext;

    #[derive(Default)]
    struct NoopSvc;

    #[allow(async_fn_in_trait)]
    impl Service<Bytes> for NoopSvc {
        type Response = Option<Bytes>;
        type Error = KernelError;

        async fn call(
            &self,
            _context: BizContext,
            _request: Bytes,
        ) -> core::result::Result<Self::Response, Self::Error> {
            Ok(None)
        }
    }

    struct NoopIo;
    impl IoOps for NoopIo {
        fn capabilities(&self) -> ChannelCaps {
            caps::STREAM
        }
        fn try_read_lease(&mut self) -> Result<ReadOutcome> {
            Err(KernelError::WouldBlock)
        }
        fn try_read_into(&mut self, _dst: &mut [u8]) -> Result<ReadOutcome> {
            Err(KernelError::WouldBlock)
        }
        fn try_write(&mut self, src: &[u8]) -> Result<usize> {
            Ok(src.len())
        }
        fn flush(&mut self) -> Result<()> {
            Ok(())
        }
        fn close(&mut self) -> Result<()> {
            Ok(())
        }
        fn rx_ptr_len(&mut self, _tok: RxToken) -> Option<(*const u8, usize)> {
            None
        }
        fn release_rx(&mut self, _tok: RxToken) {}
    }
    impl DynChannel for NoopIo {
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    #[derive(Default)]
    struct TestCtx {
        exception_count: usize,
        close_count: usize,
        backpressure_false: usize,
    }
    impl<A> ChannelHandlerContext<A> for TestCtx {
        fn fire_channel_active(&mut self) -> Result<()> {
            Ok(())
        }
        fn fire_channel_inactive(&mut self) -> Result<()> {
            Ok(())
        }
        fn fire_channel_read_raw(
            &mut self,
            _boundary: crate::io::MsgBoundary,
            _bytes: Bytes,
        ) -> Result<()> {
            Ok(())
        }
        fn fire_channel_read(&mut self, _msg: Bytes) -> Result<()> {
            Ok(())
        }
        fn fire_channel_read_complete(&mut self) -> Result<()> {
            Ok(())
        }
        fn fire_exception_caught(&mut self, _err: KernelError) -> Result<()> {
            self.exception_count += 1;
            Ok(())
        }
        fn fire_channel_writability_changed(&mut self, is_writable: bool) -> Result<()> {
            if !is_writable {
                self.backpressure_false += 1;
            }
            Ok(())
        }
        fn fire_app_complete(
            &mut self,
            _result: core::result::Result<Option<Bytes>, KernelError>,
        ) -> Result<()> {
            Ok(())
        }
        fn write(&mut self, _msg: OutboundFrame) -> Result<()> {
            Ok(())
        }
        fn flush(&mut self) -> Result<()> {
            Ok(())
        }
        fn close(&mut self) -> Result<()> {
            self.close_count += 1;
            Ok(())
        }
    }

    fn state() -> ChannelState<NoopEvidenceSink, NoopIo> {
        ChannelState::new(
            1,
            NoopIo,
            1024,
            512,
            usize::MAX,
            FlushPolicy::default().budget(1024),
            NoopEvidenceSink,
        )
    }

    #[test]
    fn queue_upper_bound_enforced_with_fail_fast() {
        let app = Arc::new(NoopSvc);
        let opts = AppServiceOptions {
            max_inflight_per_connection: 1,
            max_queue_per_connection: 1,
            overload_action: OverloadAction::FailFast,
        };
        let mut h = AppServiceHandler::new(app, opts);
        let mut ctx = TestCtx::default();
        let mut st = state();

        let _ = h.channel_read(&mut ctx, &mut st, Bytes::from_static(b"a"));
        let _ = h.channel_read(&mut ctx, &mut st, Bytes::from_static(b"b"));
        let _ = h.channel_read(&mut ctx, &mut st, Bytes::from_static(b"c"));

        assert_eq!(ctx.exception_count, 1);
        let stats = h.take_overload_stats();
        assert_eq!(stats.reject_total, 1);
        assert_eq!(stats.queue_high_watermark, 1);
    }

    #[test]
    fn overload_backpressure_signal_emitted() {
        let app = Arc::new(NoopSvc);
        let opts = AppServiceOptions {
            max_inflight_per_connection: 1,
            max_queue_per_connection: 0,
            overload_action: OverloadAction::Backpressure,
        };
        let mut h = AppServiceHandler::new(app, opts);
        let mut ctx = TestCtx::default();
        let mut st = state();

        let _ = h.channel_read(&mut ctx, &mut st, Bytes::from_static(b"a"));
        let _ = h.channel_read(&mut ctx, &mut st, Bytes::from_static(b"b"));

        assert_eq!(ctx.backpressure_false, 1);
        let stats = h.take_overload_stats();
        assert_eq!(stats.backpressure_total, 1);
    }

    #[test]
    fn overload_can_close_connection() {
        let app = Arc::new(NoopSvc);
        let opts = AppServiceOptions {
            max_inflight_per_connection: 1,
            max_queue_per_connection: 0,
            overload_action: OverloadAction::CloseConnection,
        };
        let mut h = AppServiceHandler::new(app, opts);
        let mut ctx = TestCtx::default();
        let mut st = state();

        let _ = h.channel_read(&mut ctx, &mut st, Bytes::from_static(b"a"));
        let _ = h.channel_read(&mut ctx, &mut st, Bytes::from_static(b"b"));

        assert_eq!(ctx.close_count, 1);
        let stats = h.take_overload_stats();
        assert_eq!(stats.close_total, 1);
    }
}
