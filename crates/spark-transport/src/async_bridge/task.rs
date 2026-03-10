use std::future::Future;
use std::pin::Pin;

use spark_buffer::Bytes;

use crate::KernelError;

// Note: we intentionally do **not** require `Send` here.
//
// `spark-core::Service` uses `async fn` in a trait, and Rust does not auto-imply
// that the returned future is `Send`. The adapter polls these futures on the
// same thread that owns the bridge.
pub(super) type AppFuture =
    Pin<Box<dyn Future<Output = core::result::Result<Option<Bytes>, KernelError>> + 'static>>;

pub(super) enum TaskState {
    New,
    App { fut: AppFuture },
}

impl core::fmt::Debug for TaskState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            TaskState::New => f.write_str("TaskState::New"),
            TaskState::App { .. } => f.write_str("TaskState::App{..}"),
        }
    }
}

#[derive(Debug)]
pub(super) struct TaskSlot {
    pub(super) chan_id: u32,
    pub(super) state: TaskState,
}
