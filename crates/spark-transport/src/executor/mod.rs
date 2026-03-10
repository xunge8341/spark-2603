//! Executor 抽象：用最小接口描述“如何调度/驱动任务”。
//!
//! Rust 对 OOP 同学的提醒：
//! - 不要把 Executor 设计成“可到处持有的对象 + 虚函数回调”。
//! - 更推荐把它当作一个**后端能力**：由运行时/线程模型实现，并注入到 driver。

use spark_uci::{Budget, KernelError, Result, TaskToken};

pub trait TaskRunner {
    fn run_token(&mut self, token: TaskToken) -> RunStatus;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    Done,
    Resubmit,
    Backpressured,
    Closed,
}

pub trait Executor {
    fn submit(&mut self, token: TaskToken, prio: u8) -> Result<()>;

    fn drive<R>(&mut self, runner: &mut R, budget: Budget) -> Result<()>
    where
        R: TaskRunner;
}

/// Convenience for stubs.
#[inline]
pub fn unsupported<T>() -> core::result::Result<T, KernelError> {
    Err(KernelError::Unsupported)
}
