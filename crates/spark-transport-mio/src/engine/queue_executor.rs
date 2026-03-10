use spark_transport::executor::{Executor, RunStatus, TaskRunner};
use spark_transport::{Budget, Result, TaskToken};

use std::collections::VecDeque;

#[derive(Debug, Default)]
pub struct QueueExecutor {
    q: VecDeque<TaskToken>,
}

impl QueueExecutor {
    pub fn new() -> Self {
        Self { q: VecDeque::new() }
    }
}

impl Executor for QueueExecutor {
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
                    // Do not requeue: wait for Writable event.
                }
            }
        }
        Ok(())
    }
}
