use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use spark_buffer::Bytes;
use spark_core::context::Context as BizContext;
use spark_core::service::Service;

use spark_transport::async_bridge::channel::ChannelLimits;
use spark_transport::async_bridge::dyn_boundary::Channel;
use spark_transport::async_bridge::{DynChannel, FrameDecoderProfile};
use spark_transport::evidence::{EvidenceSink, NoopEvidenceSink};
use spark_transport::io::{caps, ChannelCaps, IoOps, MsgBoundary, ReadData, ReadOutcome, RxToken};
use spark_transport::policy::FlushPolicy;
use spark_transport::{DataPlaneMetrics, KernelError, Result};

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

fn noop_waker() -> Waker {
    unsafe fn clone(_: *const ()) -> RawWaker {
        RawWaker::new(core::ptr::null(), &VTABLE)
    }
    unsafe fn wake(_: *const ()) {}
    unsafe fn wake_by_ref(_: *const ()) {}
    unsafe fn drop(_: *const ()) {}

    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);
    unsafe { Waker::from_raw(RawWaker::new(core::ptr::null(), &VTABLE)) }
}

fn poll_to_ready<T>(mut fut: Pin<&mut T>) -> T::Output
where
    T: core::future::Future,
{
    let waker = noop_waker();
    let mut cx = Context::from_waker(&waker);
    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(v) => return v,
            Poll::Pending => continue,
        }
    }
}

#[derive(Debug)]
struct LeaseMemChannel {
    read: Vec<u8>,
    lease_supported: bool,
    leased_once: bool,
    released: u64,
    closed: bool,
}

impl LeaseMemChannel {
    fn with_input(bytes: &[u8], lease_supported: bool) -> Self {
        Self {
            read: bytes.to_vec(),
            lease_supported,
            leased_once: false,
            released: 0,
            closed: false,
        }
    }
}

impl IoOps for LeaseMemChannel {
    fn capabilities(&self) -> ChannelCaps {
        caps::STREAM
    }

    fn try_read_lease(&mut self) -> Result<ReadOutcome> {
        if self.closed {
            return Err(KernelError::Closed);
        }
        if !self.lease_supported {
            return Err(KernelError::Unsupported);
        }
        if self.leased_once || self.read.is_empty() {
            return Err(KernelError::WouldBlock);
        }
        self.leased_once = true;
        Ok(ReadOutcome {
            n: self.read.len(),
            boundary: MsgBoundary::None,
            truncated: false,
            data: ReadData::Token(RxToken((1u64 << 32) | 1)),
        })
    }

    fn try_read_into(&mut self, dst: &mut [u8]) -> Result<ReadOutcome> {
        if self.closed {
            return Err(KernelError::Closed);
        }
        if self.read.is_empty() {
            return Err(KernelError::WouldBlock);
        }
        let n = self.read.len().min(dst.len());
        dst[..n].copy_from_slice(&self.read[..n]);
        self.read.drain(..n);
        Ok(ReadOutcome {
            n,
            boundary: MsgBoundary::None,
            truncated: false,
            data: ReadData::Copied,
        })
    }

    fn try_write(&mut self, src: &[u8]) -> Result<usize> {
        if self.closed {
            return Err(KernelError::Closed);
        }
        Ok(src.len())
    }

    fn flush(&mut self) -> Result<()> {
        if self.closed {
            return Err(KernelError::Closed);
        }
        Ok(())
    }

    fn close(&mut self) -> Result<()> {
        self.closed = true;
        Ok(())
    }

    fn rx_ptr_len(&mut self, _tok: RxToken) -> Option<(*const u8, usize)> {
        Some((self.read.as_ptr(), self.read.len()))
    }

    fn release_rx(&mut self, _tok: RxToken) {
        self.released = self.released.saturating_add(1);
        self.read.clear();
    }
}

impl spark_transport::async_bridge::dyn_channel::DynChannel for LeaseMemChannel {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

fn make_channel(io: LeaseMemChannel) -> Channel<NoopService> {
    let io_box: Box<dyn DynChannel> = Box::new(io);
    let sink: Arc<dyn EvidenceSink> = Arc::new(NoopEvidenceSink);
    let app = Arc::new(NoopService);
    let limits = ChannelLimits::new(64 * 1024, 1024 * 1024, 512 * 1024);
    let flush = FlushPolicy::default().budget(limits.max_frame);
    Channel::new_with_profile_and_flush_budget(
        1,
        io_box,
        FrameDecoderProfile::line(64 * 1024),
        limits,
        flush,
        app,
        sink,
    )
}

#[test]
fn leased_stream_counts_and_releases_once() {
    let mut ch = make_channel(LeaseMemChannel::with_input(b"ping\n", true));
    let mut read_buf = vec![0u8; 64];

    let (read_bytes, _, _, _, _, _, cumulation_copy_bytes, lease_tokens, lease_borrowed, materialize) =
        ch.on_readable(&mut read_buf, 8).expect("on_readable");

    assert_eq!(read_bytes, 5);
    assert_eq!(lease_tokens, 1);
    assert_eq!(lease_borrowed, 5);
    assert_eq!(materialize, 0);
    assert_eq!(cumulation_copy_bytes, 5);

    if let Some(mut fut) = ch.take_app_future() {
        let out = poll_to_ready(Pin::new(&mut fut));
        ch.on_app_complete(out);
    }

    let io = ch
        .io_mut()
        .as_any_mut()
        .downcast_mut::<LeaseMemChannel>()
        .expect("lease mem io");
    assert_eq!(io.released, 1);
}

#[test]
fn unsupported_lease_path_has_no_lease_counters() {
    let mut ch = make_channel(LeaseMemChannel::with_input(b"ping\n", false));
    let mut read_buf = vec![0u8; 64];

    let (read_bytes, _, _, _, _, _, cumulation_copy_bytes, lease_tokens, lease_borrowed, materialize) =
        ch.on_readable(&mut read_buf, 8).expect("on_readable");

    assert_eq!(read_bytes, 5);
    assert_eq!(lease_tokens, 0);
    assert_eq!(lease_borrowed, 0);
    assert_eq!(materialize, 0);
    assert_eq!(cumulation_copy_bytes, 5);
}

#[test]
fn metrics_expose_phase_a_rx_counters() {
    let metrics = DataPlaneMetrics::default();
    metrics.record_rx_lease(2, 10);
    metrics.record_rx_materialize(4);
    metrics.record_rx_cumulation_copy(6);

    let snap = metrics.snapshot();
    assert_eq!(snap.rx_lease_tokens_total, 2);
    assert_eq!(snap.rx_lease_borrowed_bytes_total, 10);
    assert_eq!(snap.rx_materialize_bytes_total, 4);
    assert_eq!(snap.rx_cumulation_copy_bytes_total, 6);
}
