//! Runtime engine implementations.
//!
//! `spark-engine-uring` will eventually host an `io_uring` reactor
//! and executors.
//!
//! For now, we provide a **LocalEngine** (in-memory reactor/executor) that is useful
//! for unit tests and bring-up of the `spark-adapter` bridge without depending on
//! OS-specific io_uring wiring.

use spark_transport::executor::{Executor, RunStatus, TaskRunner};
use spark_transport::reactor::{Interest, KernelEvent, Reactor};
use spark_transport::{Budget, KernelError, Result, TaskToken};
use std::collections::VecDeque;
use std::mem::MaybeUninit;

/// In-memory reactor backed by a queue.
#[derive(Debug, Default)]
pub struct LocalReactor {
    q: VecDeque<KernelEvent>,
}

impl LocalReactor {
    pub fn new() -> Self {
        Self { q: VecDeque::new() }
    }

    pub fn push(&mut self, ev: KernelEvent) {
        self.q.push_back(ev);
    }
}

impl Reactor for LocalReactor {
    fn poll_into(
        &mut self,
        budget: Budget,
        out: &mut [MaybeUninit<KernelEvent>],
    ) -> Result<usize> {
        let max = budget
            .max_events
            .min(out.len() as u32) as usize;
        let mut n = 0;
        while n < max {
            match self.q.pop_front() {
                Some(ev) => {
                    out[n].write(ev);
                    n += 1;
                }
                None => break,
            }
        }
        Ok(n)
    }

    fn register(&mut self, _chan_id: u32, _interest: Interest) -> core::result::Result<(), KernelError> {
        Ok(())
    }
}

/// In-memory executor backed by a queue.
#[derive(Debug, Default)]
pub struct LocalExecutor {
    q: VecDeque<TaskToken>,
}

impl LocalExecutor {
    pub fn new() -> Self {
        Self { q: VecDeque::new() }
    }
}

impl Executor for LocalExecutor {
    fn submit(&mut self, token: TaskToken, _prio: u8) -> Result<()> {
        self.q.push_back(token);
        Ok(())
    }

    fn drive<R>(&mut self, runner: &mut R, budget: Budget) -> Result<()>
    where
        R: TaskRunner,
    {
        let max = budget.max_events.max(1) as usize;
        for _ in 0..max {
            let Some(tok) = self.q.pop_front() else { break; };
            match runner.run_token(tok) {
                RunStatus::Done | RunStatus::Closed => {}
                RunStatus::Resubmit => self.q.push_back(tok),
                RunStatus::Backpressured => {
                    // Do not busy requeue; wait for a Writable (or other) event.
                }
            }
        }
        Ok(())
    }
}

/// A convenience bundle.
#[derive(Debug, Default)]
pub struct LocalEngine {
    pub reactor: LocalReactor,
    pub executor: LocalExecutor,
}

impl LocalEngine {
    pub fn new() -> Self {
        Self { reactor: LocalReactor::new(), executor: LocalExecutor::new() }
    }
}
