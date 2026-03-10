use core::mem::MaybeUninit;

use super::KernelEvent;

/// Read-only view over a `MaybeUninit<KernelEvent>` buffer.
///
/// # Safety model
///
/// `Reactor::poll_into` writes a contiguous prefix of initialized events into an
/// `out: &mut [MaybeUninit<KernelEvent>]` buffer and returns `n`.
///
/// This helper turns that contract into a safe iterator.
/// The only `unsafe` in this module is the `assume_init_read` used to read the
/// initialized prefix.
///
/// # Tests
///
/// - `tests::iter_init_reads_exact_prefix` ensures we only read the initialized prefix.
pub(crate) fn iter_init<'a>(buf: &'a [MaybeUninit<KernelEvent>], n: usize) -> impl Iterator<Item = KernelEvent> + 'a {
    let n = n.min(buf.len());
    buf.iter().take(n).map(|slot| {
        // Safety:
        // - by `Reactor::poll_into` contract, the first `n` entries are initialized;
        // - `KernelEvent: Copy`, so `assume_init_read` is a by-value read.
        unsafe { slot.assume_init_read() }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::MaybeUninit;

    #[test]
    fn iter_init_reads_exact_prefix() {
        let mut out = [MaybeUninit::<KernelEvent>::uninit(); 4];
        out[0] = MaybeUninit::new(KernelEvent::Readable { chan_id: 1 });
        out[1] = MaybeUninit::new(KernelEvent::Writable { chan_id: 2 });

        let evs: Vec<KernelEvent> = iter_init(&out, 2).collect();
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0], KernelEvent::Readable { chan_id: 1 });
        assert_eq!(evs[1], KernelEvent::Writable { chan_id: 2 });
    }
}
