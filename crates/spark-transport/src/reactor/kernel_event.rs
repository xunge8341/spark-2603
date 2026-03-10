use core::fmt;

/// Kernel events produced by a [`crate::Reactor`].
///
/// Design note:
/// - Readiness-based backends (mio/epoll/kqueue) only know *"readable"*.
/// - Completion-based backends (io_uring) may optionally attach extra metadata.
///
/// Therefore `Readable` does **not** carry an `RxToken`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum KernelEvent {
    /// Readable event.
    Readable { chan_id: u32 },
    /// Writable event.
    Writable { chan_id: u32 },
    /// Timer fired.
    Timer { token: u64 },
    /// Channel closed.
    Closed { chan_id: u32 },
}

impl fmt::Debug for KernelEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            KernelEvent::Readable { chan_id } => f
                .debug_struct("Readable")
                .field("chan_id", &chan_id)
                .finish(),
            KernelEvent::Writable { chan_id } => f
                .debug_struct("Writable")
                .field("chan_id", &chan_id)
                .finish(),
            KernelEvent::Timer { token } => f
                .debug_struct("Timer")
                .field("token", &token)
                .finish(),
            KernelEvent::Closed { chan_id } => f
                .debug_struct("Closed")
                .field("chan_id", &chan_id)
                .finish(),
        }
    }
}
