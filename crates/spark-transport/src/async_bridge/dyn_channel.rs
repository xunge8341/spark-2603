use std::any::Any;

use crate::io::{ChannelCaps, IoOps, ReadOutcome, Result, RxToken};

/// Object-safe channel wrapper that supports downcasting.
///
/// Design note:
/// - This is an internal bring-up escape hatch.
/// - Long-term, semantic `Channel` objects should hide concrete IO types without downcast.
pub trait DynChannel: IoOps + Send {
    fn as_any_mut(&mut self) -> &mut dyn Any;
}


// -----------------------------------------------------------------------------
// Dev-friendly default: allow using `Box<dyn DynChannel>` as the default IO type
// in generic channels/drivers without forcing callers to care about indirection.
//
// Design notes:
// - `Box<dyn DynChannel>` does NOT implement `IoOps` by default.
// - We provide a small delegation impl so generic code can treat boxed dyn
//   channels the same as concrete channels.
// - This keeps the rest of the transport code generic over `Io: DynChannel`.

impl IoOps for Box<dyn DynChannel> {
    #[inline]
    fn capabilities(&self) -> ChannelCaps {
        self.as_ref().capabilities()
    }

    #[inline]
    fn try_read_lease(&mut self) -> Result<ReadOutcome> {
        self.as_mut().try_read_lease()
    }

    #[inline]
    fn try_read_into(&mut self, dst: &mut [u8]) -> Result<ReadOutcome> {
        self.as_mut().try_read_into(dst)
    }

    #[inline]
    fn try_write(&mut self, data: &[u8]) -> Result<usize> {
        self.as_mut().try_write(data)
    }

    #[inline]
    fn try_write_vectored(&mut self, bufs: &[&[u8]]) -> Result<usize> {
        self.as_mut().try_write_vectored(bufs)
    }

    #[inline]
    fn flush(&mut self) -> Result<()> {
        self.as_mut().flush()
    }

    #[inline]
    fn rx_ptr_len(&mut self, tok: RxToken) -> Option<(*const u8, usize)> {
        self.as_mut().rx_ptr_len(tok)
    }

    #[inline]
    fn release_rx(&mut self, tok: RxToken) {
        self.as_mut().release_rx(tok)
    }

    #[inline]
    fn close(&mut self) -> Result<()> {
        self.as_mut().close()
    }
}

impl DynChannel for Box<dyn DynChannel> {
    #[inline]
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self.as_mut().as_any_mut()
    }
}
