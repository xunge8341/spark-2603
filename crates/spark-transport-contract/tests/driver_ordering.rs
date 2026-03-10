use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use spark_buffer::Bytes;
use spark_core::context::Context;
use spark_core::service::Service;
use spark_transport::async_bridge::ChannelDriver;
use spark_transport::evidence::EvidenceSink;
use spark_transport::io::{caps, ChannelCaps, IoOps, MsgBoundary, ReadData, ReadOutcome};
use spark_transport::metrics::DataPlaneMetrics;
use spark_transport::reactor::{Interest, KernelEvent, Reactor};
use spark_transport::uci::names::evidence as ev_names;
use spark_transport::{Budget, KernelError, Result, TaskToken};

use spark_transport_contract::suite::CapturingSink;

#[derive(Default)]
struct RecordingReactor {
    q: VecDeque<KernelEvent>,
    regs: Vec<(u32, Interest)>,
}

impl RecordingReactor {
    fn push(&mut self, ev: KernelEvent) {
        self.q.push_back(ev);
    }

    fn take_regs(&mut self) -> Vec<(u32, Interest)> {
        std::mem::take(&mut self.regs)
    }
}

impl Reactor for RecordingReactor {
    fn poll_into(
        &mut self,
        budget: Budget,
        out: &mut [core::mem::MaybeUninit<KernelEvent>],
    ) -> Result<usize> {
        let mut n = 0usize;
        let limit = budget.max_events.max(1) as usize;
        while n < limit {
            let Some(ev) = self.q.pop_front() else {
                break;
            };
            if n >= out.len() {
                break;
            }
            out[n].write(ev);
            n += 1;
        }
        Ok(n)
    }

    fn register(&mut self, chan_id: u32, interest: Interest) -> core::result::Result<(), KernelError> {
        self.regs.push((chan_id, interest));
        Ok(())
    }
}

#[derive(Default)]
struct InlineExecutor {
    q: VecDeque<TaskToken>,
}

impl spark_transport::executor::Executor for InlineExecutor {
    fn submit(&mut self, token: TaskToken, _prio: u8) -> Result<()> {
        self.q.push_back(token);
        Ok(())
    }

    fn drive<R>(&mut self, runner: &mut R, budget: Budget) -> Result<()>
    where
        R: spark_transport::executor::TaskRunner,
    {
        let limit = budget.max_events.max(1) as usize;
        for _ in 0..limit {
            let Some(tok) = self.q.pop_front() else {
                break;
            };
            match runner.run_token(tok) {
                spark_transport::executor::RunStatus::Resubmit => self.q.push_back(tok),
                spark_transport::executor::RunStatus::Backpressured => self.q.push_back(tok),
                spark_transport::executor::RunStatus::Done
                | spark_transport::executor::RunStatus::Closed => {}
            }
        }
        Ok(())
    }
}

/// Minimal stream IO for deterministic driver tests.
struct QueuedIo {
    rx: VecDeque<Vec<u8>>,
    write_allowance: usize,
    closed: bool,
}

impl QueuedIo {
    fn new() -> Self {
        Self {
            rx: VecDeque::new(),
            write_allowance: 0,
            closed: false,
        }
    }

    fn push_rx(&mut self, bytes: Vec<u8>) {
        self.rx.push_back(bytes);
    }
}

impl IoOps for QueuedIo {
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
        let Some(front) = self.rx.front_mut() else {
            return Err(KernelError::WouldBlock);
        };

        let n = front.len().min(dst.len());
        dst[..n].copy_from_slice(&front[..n]);
        front.drain(..n);
        if front.is_empty() {
            self.rx.pop_front();
        }

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
        if self.write_allowance == 0 {
            return Err(KernelError::WouldBlock);
        }
        let n = src.len().min(self.write_allowance).min(4096);
        self.write_allowance -= n;
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

impl spark_transport::async_bridge::dyn_channel::DynChannel for QueuedIo {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

struct BigRespService {
    resp: Bytes,
}

impl Service<Bytes> for BigRespService {
    type Response = Option<Bytes>;
    type Error = KernelError;

    async fn call(&self, _context: Context, _request: Bytes) -> core::result::Result<Self::Response, Self::Error> {
        Ok(Some(self.resp.clone()))
    }
}

#[test]
fn driver_interest_switch_follows_backpressure_enter() {
    let sink = Arc::new(CapturingSink::default());
    let evidence: Arc<dyn EvidenceSink> = sink.clone();

    let resp = Bytes::from(vec![0u8; 16 * 1024]);
    let app = Arc::new(BigRespService { resp });
    let metrics = Arc::new(DataPlaneMetrics::default());

    let reactor = RecordingReactor::default();
    let executor = InlineExecutor::default();

    let mut driver: ChannelDriver<RecordingReactor, InlineExecutor, BigRespService, Arc<dyn EvidenceSink>, QueuedIo> =
        ChannelDriver::new(reactor, executor, app, 16, 1024, metrics, evidence);

    let chan_id = driver.alloc_chan_id().unwrap();
    let mut io = QueuedIo::new();
    io.push_rx(b"ping\n".to_vec());
    driver.install_channel(chan_id, io).unwrap();

    // one readable event should drive: read -> decode -> app -> enqueue outbound -> backpressure.
    driver.reactor_mut().push(KernelEvent::Readable { chan_id });
    driver.tick(Budget { max_events: 64, max_nanos: 0 }).unwrap();

    let regs = driver.reactor_mut().take_regs();
    let last = regs
        .iter()
        .rev()
        .find(|(id, _)| *id == chan_id)
        .map(|(_, i)| *i)
        .expect("expected at least one register call");

    assert!(last.contains(Interest::WRITE), "expected WRITE after backpressure");
    assert!(!last.contains(Interest::READ), "expected READ suppressed after backpressure");

    let evs = sink.take();
    assert!(evs.iter().any(|e| e.name == ev_names::BACKPRESSURE_ENTER));

}

#[test]
fn driver_interest_switches_back_to_read_after_drain() {
    let sink = Arc::new(CapturingSink::default());
    let evidence: Arc<dyn EvidenceSink> = sink.clone();

    // Large response to enter backpressure quickly.
    let resp = Bytes::from(vec![0u8; 16 * 1024]);
    let app = Arc::new(BigRespService { resp });
    let metrics = Arc::new(DataPlaneMetrics::default());

    let reactor = RecordingReactor::default();
    let executor = InlineExecutor::default();

    let mut driver: ChannelDriver<RecordingReactor, InlineExecutor, BigRespService, Arc<dyn EvidenceSink>, QueuedIo> =
        ChannelDriver::new(reactor, executor, app, 16, 1024, metrics, evidence);

    let chan_id = driver.alloc_chan_id().unwrap();
    let mut io = QueuedIo::new();
    io.push_rx(b"ping\n".to_vec());
    driver.install_channel(chan_id, io).unwrap();

    // Step 1: Readable -> enqueue outbound -> backpressure enter => READ suppressed.
    driver.reactor_mut().push(KernelEvent::Readable { chan_id });
    driver.tick(Budget { max_events: 64, max_nanos: 0 }).unwrap();

    let regs = driver.reactor_mut().take_regs();
    let last = regs
        .iter()
        .rev()
        .find(|(id, _)| *id == chan_id)
        .map(|(_, i)| *i)
        .expect("expected register call");
    assert!(last.contains(Interest::WRITE));
    assert!(!last.contains(Interest::READ));

    // Clear evidence queue before the next step.
    let _ = sink.take();

    // Step 2: Allow writes and inject Writable => outbound drains => backpressure exit => READ resumes.
    driver.__with_reactor_and_channels(|_r, chans| {
        let ch = chans[spark_transport::async_bridge::chan_index(chan_id)].as_mut().unwrap();
        ch.io_mut().write_allowance = 1024 * 1024;
    });

    driver.reactor_mut().push(KernelEvent::Writable { chan_id });
    driver.tick(Budget { max_events: 64, max_nanos: 0 }).unwrap();

    let regs = driver.reactor_mut().take_regs();
    let last = regs
        .iter()
        .rev()
        .find(|(id, _)| *id == chan_id)
        .map(|(_, i)| *i)
        .expect("expected register call after drain");
    assert!(last.contains(Interest::READ), "expected READ after drain");
    assert!(
        !last.contains(Interest::WRITE),
        "expected no WRITE after drain when backlog cleared"
    );

    let evs = sink.take();
    assert!(evs.iter().any(|e| e.name == "BackpressureExit"));
}

#[test]
fn draining_interest_becomes_empty_without_backlog() {
    let sink = Arc::new(CapturingSink::default());
    let evidence: Arc<dyn EvidenceSink> = sink.clone();

    let app = Arc::new(BigRespService {
        resp: Bytes::from_static(b""),
    });
    let metrics = Arc::new(DataPlaneMetrics::default());

    let reactor = RecordingReactor::default();
    let executor = InlineExecutor::default();

    let mut driver: ChannelDriver<RecordingReactor, InlineExecutor, BigRespService, Arc<dyn EvidenceSink>, QueuedIo> =
        ChannelDriver::new(reactor, executor, app, 16, 1024, metrics, evidence);

    let chan_id = driver.alloc_chan_id().unwrap();
    driver.install_channel(chan_id, QueuedIo::new()).unwrap();

    // enter draining (no backlog) -> interest should become empty.
    driver.__with_reactor_and_channels(|_r, chans| {
        let ch = chans[spark_transport::async_bridge::chan_index(chan_id)].as_mut().unwrap();
        ch.enter_draining(false, Duration::from_millis(10), 0);
    });
    driver.tick(Budget { max_events: 64, max_nanos: 0 }).unwrap();

    let regs = driver.reactor_mut().take_regs();
    let last = regs
        .iter()
        .rev()
        .find(|(id, _)| *id == chan_id)
        .map(|(_, i)| *i)
        .expect("expected register call");
    assert!(last.is_empty(), "expected empty interest when draining without backlog");

    let evs = sink.take();
    assert!(evs.iter().any(|e| e.name == ev_names::DRAINING_ENTER));
}

#[test]
fn driver_draining_timeout_emits_and_interest_empty() {
    let sink = Arc::new(CapturingSink::default());
    let evidence: Arc<dyn EvidenceSink> = sink.clone();

    let app = Arc::new(BigRespService {
        resp: Bytes::from_static(b""),
    });
    let metrics = Arc::new(DataPlaneMetrics::default());

    let reactor = RecordingReactor::default();
    let executor = InlineExecutor::default();

    let mut driver: ChannelDriver<RecordingReactor, InlineExecutor, BigRespService, Arc<dyn EvidenceSink>, QueuedIo> =
        ChannelDriver::new(reactor, executor, app, 16, 1024, metrics, evidence);

    let chan_id = driver.alloc_chan_id().unwrap();
    driver.install_channel(chan_id, QueuedIo::new()).unwrap();

    // Enter draining with a short timeout. No backlog should imply empty interest.
    driver.__with_reactor_and_channels(|_r, chans| {
        let ch = chans[spark_transport::async_bridge::chan_index(chan_id)].as_mut().unwrap();
        ch.enter_draining(false, Duration::from_millis(10), 0);
    });

    std::thread::sleep(Duration::from_millis(25));
    driver.tick(Budget { max_events: 64, max_nanos: 0 }).unwrap();

    let regs = driver.reactor_mut().take_regs();
    let last = regs
        .iter()
        .rev()
        .find(|(id, _)| *id == chan_id)
        .map(|(_, i)| *i)
        .expect("expected register call");
    assert!(
        last.is_empty(),
        "expected empty interest on draining timeout without backlog"
    );

    let evs = sink.take();
    assert!(evs.iter().any(|e| e.name == ev_names::DRAINING_TIMEOUT));
}
