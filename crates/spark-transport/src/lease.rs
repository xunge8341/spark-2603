use crate::{KernelError, Result, RxToken};
use core::marker::PhantomData;
use std::rc::Rc;

/// Registry responsible for mapping `RxToken` to a borrowed buffer view.
pub trait LeaseRegistry {
    fn borrow_rx<'a>(&'a mut self, tok: RxToken) -> Result<RxLease<'a, Self>>
    where
        Self: Sized;

    /// Release a previously borrowed RX token.
    ///
    /// # Contract
    ///
    /// This is intended to be called only from `RxLease::drop` (the unique return point).
    /// After this call, `tok` must be treated as invalid and must not be used again.
    fn release_rx(&mut self, tok: RxToken);
}

/// Pointer-based borrowed buffer guard.
///
/// This keeps raw pointer + len to avoid aliasing pitfalls during drop.
pub struct RxLease<'a, LR: LeaseRegistry + ?Sized> {
    ptr: *const u8,
    len: usize,
    tok: RxToken,
    reg: *mut LR,
    // 重要：RxLease 不能跨线程移动。
    // 原因：drop 时会回调 `LR::release_rx`，该 registry 指针通常只在驱动线程有效。
    // 用 `Rc` 阻止 auto trait 派生 Send/Sync（Rc: !Send + !Sync）。
    _nosend: PhantomData<Rc<()>>,
    _lt: PhantomData<&'a mut LR>,
}

impl<'a, LR: LeaseRegistry + ?Sized> RxLease<'a, LR> {
    /// Construct a new lease.
    ///
    /// This is a *safe* constructor, but it relies on the `LeaseRegistry::borrow_rx` contract:
    /// the returned `(ptr, len)` pair must remain valid for reads until the lease is dropped.
    pub fn new(ptr: *const u8, len: usize, tok: RxToken, reg: *mut LR) -> Self {
        Self {
            ptr,
            len,
            tok,
            reg,
            _nosend: PhantomData,
            _lt: PhantomData,
        }
    }

    #[inline]
    pub fn bytes(&self) -> &'a [u8] {
        // SAFETY:
        // - `ptr..ptr+len` remains valid for reads until this lease is dropped;
        // - this is guaranteed by the `LeaseRegistry::borrow_rx` contract.
        unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
    }

    #[inline]
    pub fn token(&self) -> RxToken {
        self.tok
    }
}

/// Copy bytes from a raw `(ptr, len)` pair.
///
/// # SAFETY model
///
/// This function assumes the pointer was obtained from a transport IO contract
/// (`rx_ptr_len`, `try_read_lease`, etc.), which guarantees that `ptr..ptr+len`
/// is valid for reads until the token is released.
///
/// The only `unsafe` is the raw slice creation, contained in this module.
///
/// # Tests
///
/// - `tests::lease_drop_releases_exactly_once` ensures the token is released exactly once.
#[inline]
pub(crate) fn copy_from_parts(ptr: *const u8, len: usize) -> Vec<u8> {
    // SAFETY: validity is guaranteed by the IO contract that produced `(ptr, len)`.
    unsafe { core::slice::from_raw_parts(ptr, len) }.to_vec()
}

impl<'a, LR: LeaseRegistry + ?Sized> Drop for RxLease<'a, LR> {
    fn drop(&mut self) {
        // SAFETY: `reg` points to a live registry for the entire lease lifetime.
        unsafe { (*self.reg).release_rx(self.tok) }
    }
}

/// Convenience error for registries that haven't been wired yet.
#[inline]
pub fn lease_failed<T>() -> core::result::Result<T, KernelError> {
    Err(KernelError::Internal(crate::error_codes::ERR_LEASE_FAILED))
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::Cell;
    use std::rc::Rc;

    struct DummyReg {
        released: Rc<Cell<u32>>,
        buf: [u8; 4],
    }

    impl DummyReg {
        fn new() -> Self {
            Self {
                released: Rc::new(Cell::new(0)),
                buf: [1, 2, 3, 4],
            }
        }
    }

    impl LeaseRegistry for DummyReg {
        fn borrow_rx<'a>(&'a mut self, tok: RxToken) -> Result<RxLease<'a, Self>>
        where
            Self: Sized,
        {
            let ptr = self.buf.as_ptr();
            let len = self.buf.len();
            let reg = self as *mut Self;
            Ok(RxLease::new(ptr, len, tok, reg))
        }

        fn release_rx(&mut self, _tok: RxToken) {
            self.released.set(self.released.get() + 1);
        }
    }

    #[test]
    fn lease_drop_releases_exactly_once() {
        let mut reg = DummyReg::new();
        let released = reg.released.clone();
        let tok = RxToken(1);
        {
            let lease = reg.borrow_rx(tok).unwrap();
            assert_eq!(lease.bytes(), &[1, 2, 3, 4]);
            assert_eq!(released.get(), 0);
        }
        assert_eq!(released.get(), 1);
    }
}
