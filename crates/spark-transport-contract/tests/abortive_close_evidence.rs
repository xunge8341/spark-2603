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
    fn push(&mut self, ev: KernelEvent) {
        self.q.push_back(ev);
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

/// IO that returns Reset on first read attempt.
struct ResetOnReadIo {
    fired: bool,
}

impl ResetOnReadIo {
    fn new() -> Self {
        Self { fired: false }
    }
}

impl IoOps for ResetOnReadIo {
    fn capabilities(&self) -> ChannelCaps {
        caps::STREAM
    }

    fn try_read_lease(&mut self) -> Result<ReadOutcome> {
        Err(KernelError::Unsupported)
    }

    fn try_read_into(&mut self, _dst: &mut [u8]) -> Result<ReadOutcome> {
        if self.fired {
            return Err(KernelError::WouldBlock);
        }
        self.fired = true;
        Err(KernelError::Reset)
    }

    fn try_write(&mut self, _src: &[u8]) -> Result<usize> {
        Err(KernelError::WouldBlock)
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }
}

impl spark_transport::async_bridge::dyn_channel::DynChannel for ResetOnReadIo {
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
fn abortive_close_emits_abortive_close_evidence() {
    let sink = Arc::new(CapturingSink::default());
    let evidence: Arc<dyn EvidenceSink> = sink.clone();

    let app = Arc::new(NoopService);
    let metrics = Arc::new(DataPlaneMetrics::default());

    let reactor = RecordingReactor::default();
    let executor = InlineExecutor::default();

    let mut driver: ChannelDriver<RecordingReactor, InlineExecutor, NoopService, Arc<dyn EvidenceSink>, ResetOnReadIo> =
        ChannelDriver::new(reactor, executor, app, 16, 1024, metrics, evidence);

    let chan_id = driver.alloc_chan_id().unwrap();
    driver.install_channel(chan_id, ResetOnReadIo::new()).unwrap();

    // Trigger a readable event; the IO returns Reset.
    driver.reactor_mut().push(KernelEvent::Readable { chan_id });
    driver.tick(Budget { max_events: 64, max_nanos: 0 }).unwrap();

    let evs = sink.take();
    assert!(evs.iter().any(|e| e.name == ev_names::CLOSE_REQUESTED), "expected CloseRequested");
    assert!(
        evs.iter().any(|e| e.name == ev_names::CLOSE_COMPLETE && e.reason == "reset"),
        "expected CloseComplete(reset), got={:?}",
        evs
    );
    assert!(
        evs.iter().any(|e| e.name == ev_names::ABORTIVE_CLOSE),
        "expected AbortiveClose evidence, got={:?}",
        evs
    );
}
