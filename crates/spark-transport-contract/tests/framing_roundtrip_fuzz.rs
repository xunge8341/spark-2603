//! Deterministic fuzz-style contract tests for symmetric framing.
//!
//! Goals:
//! - Validate encode/decode roundtrip under random payloads.
//! - Validate robustness under segmented reads (cumulation) and partial writes.
//! - Validate outbound encoders are idempotent (no double-append).

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

// ---------------- deterministic rng ----------------

#[derive(Debug, Clone, Copy)]
struct XorShift64 {
    state: u64,
}

impl XorShift64 {
    fn new(seed: u64) -> Self {
        Self { state: seed.max(1) }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    fn gen_usize(&mut self, lo: usize, hi_exclusive: usize) -> usize {
        debug_assert!(lo < hi_exclusive);
        let span = (hi_exclusive - lo) as u64;
        (lo as u64 + (self.next_u64() % span)) as usize
    }

    fn fill(&mut self, buf: &mut [u8]) {
        for b in buf {
            *b = (self.next_u64() & 0xFF) as u8;
        }
    }
}

// ---------------- test app(s) ----------------

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

#[derive(Debug)]
struct FixedReplyService {
    reply: Bytes,
}

impl FixedReplyService {
    fn new(reply: &'static [u8]) -> Self {
        Self {
            reply: Bytes::from(reply.to_vec()),
        }
    }
}

#[allow(async_fn_in_trait)]
impl Service<Bytes> for FixedReplyService {
    type Response = Option<Bytes>;
    type Error = KernelError;

    async fn call(
        &self,
        _context: BizContext,
        _request: Bytes,
    ) -> core::result::Result<Self::Response, Self::Error> {
        Ok(Some(self.reply.clone()))
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
    fn new(max_read_chunk: usize, max_write_chunk: usize) -> Self {
        Self {
            read: Vec::new(),
            roff: 0,
            written: Vec::new(),
            max_read_chunk: max_read_chunk.max(1),
            max_write_chunk: max_write_chunk.max(1),
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

fn drive_one_roundtrip<A>(
    profile: FrameDecoderProfile,
    app: Arc<A>,
    wire_in: &[u8],
    wire_out: &[u8],
    read_chunk: usize,
    write_chunk: usize,
) where
    A: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
{
    let mut io = MemStreamChannel::new(read_chunk, write_chunk);
    io.feed_read(wire_in);

    let io_box: Box<dyn DynChannel> = Box::new(io);

    let sink = Arc::new(CapturingSink::default());
    let sink_erased: Arc<dyn EvidenceSink> = sink.clone();

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

    let mut read_buf = vec![0u8; read_chunk.max(1)];
    // Drive the channel until all inbound bytes have been consumed, all app work is completed,
    // and outbound has been fully flushed.
    //
    // NOTE: delimiter/line framing may split the incoming wire into multiple frames. The contract
    // is that the full wire roundtrips when the channel is fully driven.
    for _ in 0..256 {
        let _ = ch.on_readable(&mut read_buf, 8192).expect("on_readable");

        let mut had_app = false;
        while let Some(mut fut) = ch.take_app_future() {
            had_app = true;
            let out = poll_to_ready(Pin::new(&mut fut));
            ch.on_app_complete(out);
        }

        // Drain outbound fully. The in-memory IO never wouldblocks, but we still bound iterations.
        for _ in 0..4096 {
            let (st, _wrote, _sys, _wv) = ch.flush_outbound();
            if matches!(st, FlushStatus::Drained | FlushStatus::WouldBlock) {
                break;
            }
        }

        let io_state = ch
            .io_mut()
            .as_any_mut()
            .downcast_mut::<MemStreamChannel>()
            .expect("mem io");

        let read_done = io_state.roff >= io_state.read.len();
        if read_done && !had_app {
            break;
        }
    }

    let io_state = ch
        .io_mut()
        .as_any_mut()
        .downcast_mut::<MemStreamChannel>()
        .expect("mem io");

    assert!(
        io_state.roff >= io_state.read.len(),
        "did not consume all input: profile={profile:?}, roff={}, len={}, read_chunk={read_chunk}, write_chunk={write_chunk}",
        io_state.roff,
        io_state.read.len(),
    );

    let written = io_state.take_written();

    assert_eq!(
        written.as_slice(),
        wire_out,
        "profile={profile:?}, read_chunk={read_chunk}, write_chunk={write_chunk}"
    );
}

fn encode_length_field_u32_be(payload: &[u8]) -> Vec<u8> {
    let mut wire = Vec::with_capacity(4 + payload.len());
    wire.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    wire.extend_from_slice(payload);
    wire
}

fn encode_varint32(payload: &[u8]) -> Vec<u8> {
    let mut hdr = [0u8; 5];
    let mut v = payload.len() as u32;
    let mut i = 0usize;
    while v >= 0x80 {
        hdr[i] = ((v as u8) & 0x7F) | 0x80;
        v >>= 7;
        i += 1;
    }
    hdr[i] = v as u8;
    let n = i + 1;

    let mut wire = Vec::with_capacity(n + payload.len());
    wire.extend_from_slice(&hdr[..n]);
    wire.extend_from_slice(payload);
    wire
}

#[test]
fn fuzz_roundtrip_random_payloads_and_segments() {
    let echo = Arc::new(EchoService);

    let mut rng = XorShift64::new(0xC0FFEE_u64);

    // Keep this bounded: contract tests must stay fast.
    for _ in 0..200 {
        let payload_len = rng.gen_usize(0, 4096);
        let mut payload = vec![0u8; payload_len];
        rng.fill(&mut payload);

        let read_chunk = rng.gen_usize(1, 17);
        let write_chunk = rng.gen_usize(1, 1024);

        // Line
        {
            let profile = FrameDecoderProfile::line(64 * 1024);
            let mut wire = payload.clone();
            wire.push(b'\n');
            drive_one_roundtrip(profile, echo.clone(), &wire, &wire, read_chunk, write_chunk);
        }

        // Delimiter CRLF
        {
            let profile =
                FrameDecoderProfile::delimiter(64 * 1024, b"\r\n", false).expect("delimiter");
            let mut wire = payload.clone();
            wire.extend_from_slice(b"\r\n");
            drive_one_roundtrip(profile, echo.clone(), &wire, &wire, read_chunk, write_chunk);
        }

        // LengthField u32 be
        {
            let profile =
                FrameDecoderProfile::length_field(64 * 1024, 4, ByteOrder::Big).expect("len");
            let wire = encode_length_field_u32_be(&payload);
            drive_one_roundtrip(profile, echo.clone(), &wire, &wire, read_chunk, write_chunk);
        }

        // Varint32
        {
            let profile = FrameDecoderProfile::varint32(64 * 1024);
            let wire = encode_varint32(&payload);
            drive_one_roundtrip(profile, echo.clone(), &wire, &wire, read_chunk, write_chunk);
        }
    }
}

#[test]
fn outbound_line_encoder_is_idempotent_when_app_already_appends_newline() {
    let app = Arc::new(FixedReplyService::new(b"pong\n"));

    let profile = FrameDecoderProfile::line(64 * 1024);
    drive_one_roundtrip(profile, app, b"ping\n", b"pong\n", 2, 8);
}

#[test]
fn outbound_delimiter_encoder_is_idempotent_when_app_already_appends_delim() {
    let app = Arc::new(FixedReplyService::new(b"pong\r\n"));

    let profile = FrameDecoderProfile::delimiter(64 * 1024, b"\r\n", false).expect("delimiter");
    drive_one_roundtrip(profile, app, b"ping\r\n", b"pong\r\n", 2, 8);
}
