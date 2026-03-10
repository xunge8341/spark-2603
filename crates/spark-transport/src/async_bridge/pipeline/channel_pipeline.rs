use std::collections::VecDeque;
use std::sync::Arc;

use spark_buffer::Bytes;
use spark_core::service::Service;

use crate::evidence::EvidenceSink;
use crate::io::MsgBoundary;
use crate::{KernelError, Result};

use super::super::channel_state::ChannelState;
use super::super::dyn_channel::DynChannel;
use super::super::task::AppFuture;
use super::super::OutboundFrame;
use super::context::EventCtx;
use super::event::PipelineEvent;
use super::frame_decoder::StreamFrameDecoderHandler;
use super::frame_encoder::StreamFrameEncoderHandler;
use super::handler::ChannelHandler;
use super::head::HeadHandler;
use super::service_handler::AppServiceHandler;
use super::tail::TailHandler;
use super::FrameDecoderProfile;
use super::HandlerVec;

/// 每连接的 `ChannelPipeline`（bring-up 版本，trampoline 驱动）。
///
/// 默认链（head -> tail）：
/// 1) HeadHandler：负责对接 outbound buffer（真正 I/O drain 由 driver 做）
/// 2) StreamFrameDecoderHandler：TCP cumulation + decode loop（语义对齐 Netty ByteToMessageDecoder）
/// 3) AppServiceHandler：业务 Service（异步）桥接
/// 4) TailHandler：兜底（丢弃未处理事件 / 关闭策略）
///
/// 本轮“更 Rust”的取舍：
/// - **核心链路静态分派**：默认 4 个 handler 以具体类型存储（无 vtable）；
/// - **扩展点仍然 dyn**：`add_first/add_last` 的额外 handler 仍使用 trait object（只在边界承担开销）。
pub struct ChannelPipeline<A, Ev = Arc<dyn EvidenceSink>, Io = Box<dyn DynChannel>>
where
    Ev: EvidenceSink,
    Io: DynChannel,
{
    // ---- core handlers (static dispatch) ----
    head: HeadHandler,
    encoder: StreamFrameEncoderHandler,
    frame: StreamFrameDecoderHandler,
    app: AppServiceHandler<A>,
    tail: TailHandler,

    // ---- optional extensions (boundary dyn) ----
    first: HandlerVec<A, Ev, Io>,
    last: HandlerVec<A, Ev, Io>,

    queue: VecDeque<PipelineEvent>,
}

impl<A, Ev, Io> core::fmt::Debug for ChannelPipeline<A, Ev, Io>
where
    Ev: EvidenceSink,
    Io: DynChannel,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ChannelPipeline")
            .field("first", &self.first.len())
            .field("last", &self.last.len())
            .finish()
    }
}

impl<A, Ev, Io> ChannelPipeline<A, Ev, Io>
where
    A: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
    Ev: EvidenceSink,
    Io: DynChannel,
{
    #[inline]
    fn handler_len(&self) -> usize {
        // head + encoder + frame + app + tail + extras
        self.first.len() + self.last.len() + 5
    }
    /// 默认 pipeline（对外 API）。
    #[allow(dead_code)]
    pub fn new_default(app: Arc<A>, max_frame: usize) -> Self {
        Self::new_with_extras(
            app,
            FrameDecoderProfile::line(max_frame),
            Vec::new(),
            Vec::new(),
        )
    }

    /// 生成 pipeline（允许插入额外 handler）。
    ///
    /// 组合顺序（head -> tail）：
    /// Head -> first... -> Encoder -> FrameDecoder -> AppService -> last... -> Tail
    pub fn new_with_extras(
        app: Arc<A>,
        profile: FrameDecoderProfile,
        first: HandlerVec<A, Ev, Io>,
        last: HandlerVec<A, Ev, Io>,
    ) -> Self {
        Self {
            head: HeadHandler,
            encoder: StreamFrameEncoderHandler::new(profile),
            frame: StreamFrameDecoderHandler::new(profile),
            app: AppServiceHandler::new(app),
            tail: TailHandler,
            first,
            last,
            queue: VecDeque::new(),
        }
    }

    #[inline]
    fn enqueue(&mut self, ev: PipelineEvent) {
        self.queue.push_back(ev);
    }

    /// trampoline：循环处理队列中的事件。
    fn pump(&mut self, state: &mut ChannelState<Ev, Io>) -> Result<()> {
        macro_rules! dispatch {
            ($start:expr, $method:ident $(, $arg:expr)* $(,)?) => {{
                let len = self.handler_len();
                if $start >= len {
                    Ok(())
                } else {
                    let mut ctx = EventCtx::<A>::new(&mut self.queue, len, $start);
                    let fl = self.first.len();
                    let idx_encoder = 1 + fl;
                    let idx_frame = idx_encoder + 1;
                    let idx_app = idx_frame + 1;
                    let idx_last_start = idx_app + 1;
                    let idx_tail = idx_last_start + self.last.len();

                    let res: Result<()> = match $start {
                        0 => self.head.$method(&mut ctx, state $(, $arg)*),
                        i if i >= 1 && i <= fl => self.first[i - 1].as_mut().$method(&mut ctx, state $(, $arg)*),
                        i if i == idx_encoder => self.encoder.$method(&mut ctx, state $(, $arg)*),
                        i if i == idx_frame => self.frame.$method(&mut ctx, state $(, $arg)*),
                        i if i == idx_app => self.app.$method(&mut ctx, state $(, $arg)*),
                        i if i < idx_tail => {
                            self.last[i - idx_last_start].as_mut().$method(&mut ctx, state $(, $arg)*)
                        }
                        _ => self.tail.$method(&mut ctx, state $(, $arg)*),
                    };

                    match res {
                        Ok(()) => Ok(()),
                        Err(err) => {
                            super::context::ChannelHandlerContext::fire_exception_caught(&mut ctx, err)?;
                            Ok(())
                        }
                    }
                }
            }};
        }

        while let Some(ev) = self.queue.pop_front() {
            match ev {
                // ---------------- inbound ----------------
                PipelineEvent::ChannelActive { start } => dispatch!(start, channel_active)?,
                PipelineEvent::ChannelInactive { start } => dispatch!(start, channel_inactive)?,
                PipelineEvent::ChannelReadRaw {
                    start,
                    boundary,
                    bytes,
                } => dispatch!(start, channel_read_raw, boundary, bytes)?,
                PipelineEvent::ChannelRead { start, msg } => dispatch!(start, channel_read, msg)?,
                PipelineEvent::ChannelReadComplete { start } => {
                    dispatch!(start, channel_read_complete)?
                }
                PipelineEvent::ExceptionCaught { start, err } => {
                    dispatch!(start, exception_caught, err)?
                }
                PipelineEvent::WritabilityChanged { start, is_writable } => {
                    dispatch!(start, channel_writability_changed, is_writable)?
                }
                PipelineEvent::AppComplete { start, result } => {
                    dispatch!(start, on_app_complete, result)?
                }

                // ---------------- outbound ----------------
                PipelineEvent::Write { start, msg } => dispatch!(start, write, msg)?,
                PipelineEvent::Flush { start } => dispatch!(start, flush)?,
                PipelineEvent::Close { start } => dispatch!(start, close)?,
            }
        }
        Ok(())
    }

    // ---------------- inbound entrypoints（对外） ----------------

    pub fn fire_channel_active(&mut self, state: &mut ChannelState<Ev, Io>) -> Result<()> {
        self.enqueue(PipelineEvent::ChannelActive { start: 0 });
        self.pump(state)
    }

    pub fn fire_channel_inactive(&mut self, state: &mut ChannelState<Ev, Io>) -> Result<()> {
        self.enqueue(PipelineEvent::ChannelInactive { start: 0 });
        self.pump(state)
    }

    pub fn fire_channel_read_raw(
        &mut self,
        state: &mut ChannelState<Ev, Io>,
        boundary: MsgBoundary,
        bytes: Bytes,
    ) -> Result<()> {
        self.enqueue(PipelineEvent::ChannelReadRaw {
            start: 0,
            boundary,
            bytes,
        });
        self.pump(state)
    }

    /// Fast-path for stream reads: accept a borrowed slice and feed it into the frame decoder.
    ///
    /// Contract:
    /// - Only valid for `MsgBoundary::None` (stream/TCP).
    /// - Borrowed slice lifetime is call-stack scoped; it must not cross queue/handler/task boundaries.
    /// - When there are pre-frame handlers (`add_first`), we fall back to the owned-bytes path
    ///   to preserve extensibility and avoid lifetime pitfalls.
    pub fn fire_channel_read_raw_stream_slice(
        &mut self,
        state: &mut ChannelState<Ev, Io>,
        bytes: &[u8],
    ) -> Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }

        if self.first.is_empty() {
            let len = self.handler_len();
            let fl = self.first.len();
            debug_assert_eq!(fl, 0);
            let idx_frame = 2 + fl; // head=0, encoder=1, frame=2 when no extras.
            let mut ctx = EventCtx::<A>::new(&mut self.queue, len, idx_frame);
            self.frame.on_stream_bytes(&mut ctx, state, bytes)?;
            self.pump(state)
        } else {
            // Preserve semantics for custom pre-frame handlers.
            self.fire_channel_read_raw(state, MsgBoundary::None, Bytes::copy_from_slice(bytes))
        }
    }

    pub fn fire_channel_read_complete(&mut self, state: &mut ChannelState<Ev, Io>) -> Result<()> {
        self.enqueue(PipelineEvent::ChannelReadComplete { start: 0 });
        self.pump(state)
    }

    pub fn fire_channel_writability_changed(
        &mut self,
        state: &mut ChannelState<Ev, Io>,
        is_writable: bool,
    ) -> Result<()> {
        self.enqueue(PipelineEvent::WritabilityChanged {
            start: 0,
            is_writable,
        });
        self.pump(state)
    }

    pub fn fire_app_complete(
        &mut self,
        state: &mut ChannelState<Ev, Io>,
        result: core::result::Result<Option<Bytes>, KernelError>,
    ) -> Result<()> {
        self.enqueue(PipelineEvent::AppComplete { start: 0, result });
        self.pump(state)
    }

    // ---------------- outbound entrypoints（对外） ----------------

    /// outbound：从 tail 开始写入（对外 API）。
    #[allow(dead_code)]
    pub fn write(&mut self, state: &mut ChannelState<Ev, Io>, msg: OutboundFrame) -> Result<()> {
        let len = self.handler_len();
        if len == 0 {
            return Ok(());
        }
        let start = len - 1;
        self.enqueue(PipelineEvent::Write { start, msg });
        self.pump(state)
    }

    /// outbound：flush（对外 API）。
    #[allow(dead_code)]
    pub fn flush(&mut self, state: &mut ChannelState<Ev, Io>) -> Result<()> {
        let len = self.handler_len();
        if len == 0 {
            return Ok(());
        }
        let start = len - 1;
        self.enqueue(PipelineEvent::Flush { start });
        self.pump(state)
    }

    /// outbound：close（对外 API）。
    #[allow(dead_code)]
    pub fn close(&mut self, state: &mut ChannelState<Ev, Io>) -> Result<()> {
        let len = self.handler_len();
        if len == 0 {
            return Ok(());
        }
        let start = len - 1;
        self.enqueue(PipelineEvent::Close { start });
        self.pump(state)
    }

    // ---------------- driver integration ----------------

    /// 从 pipeline 取走一个待 poll 的业务 future。
    pub fn take_app_future(&mut self) -> Option<AppFuture> {
        // Preserve ordering semantics similar to the old flat handler array:
        // head -> first -> frame -> app -> last -> tail
        for h in self.first.iter_mut() {
            if let Some(f) = h.take_app_future() {
                return Some(f);
            }
        }

        if let Some(f) = self.app.take_app_future() {
            return Some(f);
        }

        for h in self.last.iter_mut() {
            if let Some(f) = h.take_app_future() {
                return Some(f);
            }
        }

        None
    }

    pub fn has_app_backlog(&self) -> bool {
        if self.first.iter().any(|h| h.has_app_backlog()) {
            return true;
        }
        if self.app.has_app_backlog() {
            return true;
        }
        self.last.iter().any(|h| h.has_app_backlog())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::async_bridge::channel::ChannelLimits;
    use crate::async_bridge::pipeline::context::ChannelHandlerContext;
    use crate::async_bridge::pipeline::ChannelPipelineBuilder;
    use crate::evidence::NoopEvidenceSink;
    use crate::io::{caps, ChannelCaps, IoOps, ReadData, ReadOutcome, RxToken};
    use crate::policy::FlushPolicy;
    use spark_core::context::Context as BizContext;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug, Default)]
    struct NoopService;

    #[allow(async_fn_in_trait)]
    impl Service<Bytes> for NoopService {
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

    #[derive(Debug)]
    struct MemIo {
        read: Vec<u8>,
        boundary: MsgBoundary,
        leased_once: bool,
    }

    impl IoOps for MemIo {
        fn capabilities(&self) -> ChannelCaps {
            caps::STREAM
        }

        fn try_read_lease(&mut self) -> Result<ReadOutcome> {
            if self.leased_once {
                return Err(KernelError::WouldBlock);
            }
            self.leased_once = true;
            Ok(ReadOutcome {
                n: self.read.len(),
                boundary: self.boundary,
                truncated: false,
                data: ReadData::Token(RxToken((1u64 << 32) | 1)),
            })
        }

        fn try_read_into(&mut self, _dst: &mut [u8]) -> Result<ReadOutcome> {
            Err(KernelError::Unsupported)
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
            Some((self.read.as_ptr(), self.read.len()))
        }

        fn release_rx(&mut self, _tok: RxToken) {
            self.read.clear();
        }
    }

    impl DynChannel for MemIo {
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    #[derive(Debug)]
    struct CountingRawHandler {
        raw_calls: Arc<AtomicUsize>,
    }

    impl CountingRawHandler {
        fn new(raw_calls: Arc<AtomicUsize>) -> Self {
            Self { raw_calls }
        }
    }

    impl<A, Ev, Io> ChannelHandler<A, Ev, Io> for CountingRawHandler
    where
        Ev: EvidenceSink,
        Io: DynChannel,
    {
        fn channel_read_raw(
            &mut self,
            ctx: &mut dyn ChannelHandlerContext<A>,
            _state: &mut ChannelState<Ev, Io>,
            boundary: MsgBoundary,
            bytes: Bytes,
        ) -> Result<()> {
            self.raw_calls.fetch_add(1, Ordering::Relaxed);
            ctx.fire_channel_read_raw(boundary, bytes)
        }
    }

    fn make_state(io: MemIo) -> ChannelState<Arc<dyn EvidenceSink>, MemIo> {
        let limits = ChannelLimits::new(64 * 1024, 1024 * 1024, 512 * 1024, usize::MAX);
        let flush = FlushPolicy::default().budget(limits.max_frame);
        ChannelState::new(
            1,
            io,
            limits.high_watermark,
            limits.low_watermark,
            limits.max_pending_write_bytes,
            flush,
            Arc::new(NoopEvidenceSink),
        )
    }

    #[test]
    fn borrowed_fast_path_skips_pre_frame_handlers() {
        let raw_calls = Arc::new(AtomicUsize::new(0));
        let app = Arc::new(NoopService);
        let mut pipeline =
            ChannelPipelineBuilder::<NoopService, Arc<dyn EvidenceSink>, MemIo>::new(app)
                .framing(FrameDecoderProfile::line(64 * 1024))
                .build();
        let mut state = make_state(MemIo {
            read: b"ping\n".to_vec(),
            boundary: MsgBoundary::None,
            leased_once: false,
        });

        pipeline
            .fire_channel_read_raw_stream_slice(&mut state, b"ping\n")
            .expect("read stream");

        assert_eq!(raw_calls.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn pre_frame_handler_forces_owned_fallback_path() {
        let raw_calls = Arc::new(AtomicUsize::new(0));
        let app = Arc::new(NoopService);
        let mut pipeline =
            ChannelPipelineBuilder::<NoopService, Arc<dyn EvidenceSink>, MemIo>::new(app)
                .add_first(CountingRawHandler::new(raw_calls.clone()))
                .framing(FrameDecoderProfile::line(64 * 1024))
                .build();
        let mut state = make_state(MemIo {
            read: b"ping\n".to_vec(),
            boundary: MsgBoundary::None,
            leased_once: false,
        });

        pipeline
            .fire_channel_read_raw_stream_slice(&mut state, b"ping\n")
            .expect("read stream");

        assert_eq!(raw_calls.load(Ordering::Relaxed), 1);
    }
}
