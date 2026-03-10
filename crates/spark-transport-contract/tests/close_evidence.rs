use std::collections::VecDeque;
use std::sync::Arc;

use spark_buffer::Bytes;
use spark_core::context::Context;
use spark_core::service::Service;
use spark_transport::async_bridge::ChannelDriver;
use spark_transport::evidence::EvidenceSink;
use spark_transport::io::{caps, ChannelCaps, IoOps, ReadOutcome};
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
                spark_transport::executor::RunStatus::Done | spark_transport::executor::RunStatus::Closed => {}
            }
        }
        Ok(())
    }
}

/// Minimal IO: no reads, close is supported.
struct MinimalIo {
    closed: bool,
}

impl MinimalIo {
    fn new() -> Self {
        Self { closed: false }
    }
}

impl IoOps for MinimalIo {
    fn capabilities(&self) -> ChannelCaps {
        caps::STREAM
    }

    fn try_read_lease(&mut self) -> Result<ReadOutcome> {
        Err(KernelError::Unsupported)
    }

    fn try_read_into(&mut self, _dst: &mut [u8]) -> Result<ReadOutcome> {
        if self.closed {
            return Err(KernelError::Closed);
        }
        Err(KernelError::WouldBlock)
    }

    fn try_write(&mut self, _src: &[u8]) -> Result<usize> {
        if self.closed {
            return Err(KernelError::Closed);
        }
        Err(KernelError::WouldBlock)
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

impl spark_transport::async_bridge::dyn_channel::DynChannel for MinimalIo {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

struct NoopService;

impl Service<Bytes> for NoopService {
    type Response = Option<Bytes>;
    type Error = KernelError;

    async fn call(&self, _context: Context, _request: Bytes) -> core::result::Result<Self::Response, Self::Error> {
        Ok(None)
    }
}

#[test]
fn close_emits_close_requested_and_close_complete() {
    let sink = Arc::new(CapturingSink::default());
    let evidence: Arc<dyn EvidenceSink> = sink.clone();

    let app = Arc::new(NoopService);
    let metrics = Arc::new(DataPlaneMetrics::default());

    let reactor = RecordingReactor::default();
    let executor = InlineExecutor::default();

    let mut driver: ChannelDriver<RecordingReactor, InlineExecutor, NoopService, Arc<dyn EvidenceSink>, MinimalIo> =
        ChannelDriver::new(reactor, executor, app, 16, 1024, metrics, evidence);

    let chan_id = driver.alloc_chan_id().unwrap();
    driver.install_channel(chan_id, MinimalIo::new()).unwrap();

    // Request close via pipeline (explicit user close).
    driver.__with_reactor_and_channels(|_r, channels| {
        let idx = spark_transport::async_bridge::chan_index(chan_id);
        let ch = channels[idx].as_mut().expect("installed channel");
        ch.close().expect("close");
    });

    // A single tick must reclaim immediately (no backlog) and emit CloseComplete.
    driver.tick(Budget { max_events: 64, max_nanos: 0 }).unwrap();

    let evs = sink.take();
    assert!(evs.iter().any(|e| e.name == ev_names::CLOSE_REQUESTED));
    assert!(
        evs.iter()
            .any(|e| e.name == ev_names::CLOSE_COMPLETE && e.reason == "requested"),
        "expected CloseComplete(requested), got={:?}",
        evs
    );

    // Best-effort: reactor should have been deregistered.
    let _regs = driver.reactor_mut().take_regs();
}
