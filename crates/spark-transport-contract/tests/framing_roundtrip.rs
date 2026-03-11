use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use spark_buffer::Bytes;
use spark_core::context::Context as BizContext;
use spark_core::service::Service;

use spark_transport::async_bridge::channel::ChannelLimits;
use spark_transport::async_bridge::contract::FlushStatus;
use spark_transport::async_bridge::dyn_boundary::Channel;
use spark_transport::async_bridge::{ByteOrder, DynChannel, FrameDecoderProfile};
use spark_transport::evidence::EvidenceSink;
use spark_transport::io::{caps, ChannelCaps, IoOps, MsgBoundary, ReadOutcome};
use spark_transport::policy::FlushPolicy;
use spark_transport::{KernelError, Result};

use spark_transport_contract::suite::CapturingSink;

// ---------------- test app ----------------

#[derive(Debug, Default)]
struct EchoService;

#[allow(async_fn_in_trait)]
impl Service<Bytes> for EchoService {
    type Response = Option<Bytes>;
    type Error = KernelError;

    async fn call(
        &self,
        _context: BizContext,
        request: Bytes,
    ) -> core::result::Result<Self::Response, Self::Error> {
        Ok(Some(request))
    }
}

// ---------------- in-memory stream channel ----------------

#[derive(Debug, Default)]
struct MemStreamChannel {
    read: Vec<u8>,
    roff: usize,
    written: Vec<u8>,
    max_read_chunk: usize,
    max_write_chunk: usize,
    closed: bool,
}

impl MemStreamChannel {
    fn new(max_read_chunk: usize) -> Self {
        Self {
            read: Vec::new(),
            roff: 0,
            written: Vec::new(),
            max_read_chunk: max_read_chunk.max(1),
            max_write_chunk: 64 * 1024,
            closed: false,
        }
    }

    fn feed_read(&mut self, bytes: &[u8]) {
        self.read.extend_from_slice(bytes);
    }

    fn take_written(&mut self) -> Vec<u8> {
        core::mem::take(&mut self.written)
    }
}

impl IoOps for MemStreamChannel {
    fn capabilities(&self) -> ChannelCaps {
        caps::STREAM
    }

    fn try_read_lease(&mut self) -> Result<ReadOutcome> {
        Err(KernelError::Unsupported)
    }

    fn try_read_into(&mut self, dst: &mut [u8]) -> Result<ReadOutcome> {
        if self.closed {
            return Err(KernelError::Closed);
        }
        if self.roff >= self.read.len() {
            return Err(KernelError::WouldBlock);
        }

        let remaining = self.read.len() - self.roff;
        let n = remaining.min(dst.len()).min(self.max_read_chunk);
        if n == 0 {
            return Err(KernelError::WouldBlock);
        }
        dst[..n].copy_from_slice(&self.read[self.roff..self.roff + n]);
        self.roff += n;

        Ok(ReadOutcome {
            n,
            boundary: MsgBoundary::None,
            truncated: false,
            data: spark_transport::io::ReadData::Copied,
        })
    }

    fn try_write(&mut self, data: &[u8]) -> Result<usize> {
        if self.closed {
            return Err(KernelError::Closed);
        }
        if data.is_empty() {
            return Ok(0);
        }
        let n = data.len().min(self.max_write_chunk).max(1);
        self.written.extend_from_slice(&data[..n]);
        Ok(n)
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
}

impl DynChannel for MemStreamChannel {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

// ---------------- tiny poll helper ----------------

fn noop_waker() -> Waker {
    // SAFETY: test-only no-op RawWaker vtable; data pointer is never dereferenced.
    unsafe fn clone(_: *const ()) -> RawWaker {
        RawWaker::new(core::ptr::null(), &VTABLE)
    }
    unsafe fn wake(_: *const ()) {}
    unsafe fn wake_by_ref(_: *const ()) {}
    unsafe fn drop(_: *const ()) {}

    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);
    // SAFETY: `VTABLE` satisfies RawWaker contract for this test no-op waker implementation.
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

fn drive_one_roundtrip(profile: FrameDecoderProfile, wire_in: &[u8], wire_out: &[u8]) {
    // Split input into small reads to validate segmented cumulation.
    for &chunk in &[1usize, 2, 3, 5, 8] {
        let mut io = MemStreamChannel::new(chunk);
        io.feed_read(wire_in);

        let io_box: Box<dyn DynChannel> = Box::new(io);

        let sink = Arc::new(CapturingSink::default());
        let sink_erased: Arc<dyn EvidenceSink> = sink.clone();

        let app = Arc::new(EchoService);
        let limits = ChannelLimits::new(64 * 1024, 1024 * 1024, 512 * 1024, usize::MAX);
        let flush = FlushPolicy::default().budget(limits.max_frame);

        let mut ch = Channel::new_with_profile_and_flush_budget(
            1,
            io_box,
            profile,
            limits,
            flush,
            app,
            sink_erased,
        );

        let mut read_buf = vec![0u8; chunk];
        let _ = ch.on_readable(&mut read_buf, 64).expect("on_readable");

        if let Some(mut fut) = ch.take_app_future() {
            let out = poll_to_ready(Pin::new(&mut fut));
            ch.on_app_complete(out);
        }

        // Drain outbound.
        for _ in 0..8 {
            let (st, _wrote, _sys, _wv) = ch.flush_outbound();
            if matches!(st, FlushStatus::Drained | FlushStatus::WouldBlock) {
                break;
            }
        }

        let written = ch
            .io_mut()
            .as_any_mut()
            .downcast_mut::<MemStreamChannel>()
            .expect("mem io")
            .take_written();

        assert_eq!(
            written.as_slice(),
            wire_out,
            "profile={profile:?}, chunk={chunk}"
        );
    }
}

#[test]
fn roundtrip_line() {
    let profile = FrameDecoderProfile::line(64 * 1024);
    drive_one_roundtrip(profile, b"ping\n", b"ping\n");
}

#[test]
fn roundtrip_delimiter_crlf() {
    let profile = FrameDecoderProfile::delimiter(64 * 1024, b"\r\n", false).expect("delimiter");
    drive_one_roundtrip(profile, b"ping\r\n", b"ping\r\n");
}

#[test]
fn roundtrip_length_field_u32_be() {
    let profile = FrameDecoderProfile::length_field(64 * 1024, 4, ByteOrder::Big).expect("len");
    let mut wire = Vec::new();
    wire.extend_from_slice(&4u32.to_be_bytes());
    wire.extend_from_slice(b"ping");
    drive_one_roundtrip(profile, &wire, &wire);
}

#[test]
fn roundtrip_varint32() {
    let profile = FrameDecoderProfile::varint32(64 * 1024);
    let mut wire = Vec::new();
    wire.push(4u8);
    wire.extend_from_slice(b"ping");
    drive_one_roundtrip(profile, &wire, &wire);
}
