//! Reactor 抽象：把“可读/可写/关闭”等内核事件统一成最小集合。
//!
//! 提示：
//! - readiness（mio/epoll）与 completion（io_uring）模型不同。
//! - 这里的 [`KernelEvent`] 不携带 RxToken，避免把完成模型强塞给 readiness。

mod interest;
mod event_buf;
mod kernel_event;
mod reactor_trait;

// Completion-style APIs are behind a feature gate to keep the bring-up surface stable.
#[cfg(feature = "completion")]
mod completion;

pub use interest::Interest;
pub use kernel_event::KernelEvent;
pub use reactor_trait::Reactor;

#[cfg(feature = "completion")]
pub use completion::{CompletionEvent, CompletionKind, CompletionReactor};

pub(crate) use event_buf::iter_init;
