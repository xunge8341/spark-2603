use core::mem::MaybeUninit;
use spark_uci::{Budget, KernelError, Result};

use super::{Interest, KernelEvent};

pub trait Reactor {
    /// Synchronous poll. Writes a contiguous prefix of `out`.
    fn poll_into(
        &mut self,
        budget: Budget,
        out: &mut [MaybeUninit<KernelEvent>],
    ) -> Result<usize>;

    /// Optional interest registration.
    fn register(&mut self, chan_id: u32, interest: Interest) -> core::result::Result<(), KernelError>;
}
