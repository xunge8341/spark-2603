//! Arc-backed immutable byte view.
//!
//! This is a minimal alternative to `bytes::Bytes` to keep the dependency graph clean.

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::ops::Deref;

/// Arc-backed immutable byte view.
///
/// Notes:
/// - Covers only the APIs needed by the current core path: `from_static/from(Vec)/copy_from_slice/len/is_empty/Deref`.
/// - Additional zero-copy APIs (`slice`, `split_to`, ...) can be added later without breaking callers.
#[derive(Clone)]
pub struct Bytes {
    inner: BytesInner,
    off: usize,
    len: usize,
}

impl PartialEq for Bytes {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.deref() == other.deref()
    }
}

impl Eq for Bytes {}

#[derive(Clone)]
enum BytesInner {
    Static(&'static [u8]),
    /// Shared owned buffer.
    ///
    /// NOTE: We store an `Arc<Vec<u8>>` (not `Arc<[u8]>`) on purpose.
    /// Converting `Vec<u8> -> Arc<[u8]>` requires allocating a new Arc header and copying bytes,
    /// which silently breaks the intended zero-copy fast paths.
    ///
    /// With `Arc<Vec<u8>>`, `Bytes::from(Vec<u8>)` is a true move with no additional byte copy.
    SharedVec(Arc<Vec<u8>>),
}

impl core::fmt::Debug for Bytes {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Bytes")
            .field("len", &self.len)
            .field("off", &self.off)
            .finish()
    }
}

impl Bytes {
    #[inline]
    pub fn from_static(s: &'static [u8]) -> Self {
        Self { inner: BytesInner::Static(s), off: 0, len: s.len() }
    }

    /// Return an empty buffer.
    #[inline]
    pub fn empty() -> Self {
        Self::from_static(&[])
    }

    #[inline]
    pub fn copy_from_slice(s: &[u8]) -> Self {
        Self::from(Vec::from(s))
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        self.deref()
    }
    /// Return a checked zero-copy sub-slice of this buffer.
    ///
    /// This never panics. When the range is invalid, it returns `None`.
    #[inline]
    pub fn try_slice(&self, off: usize, len: usize) -> Option<Self> {
        if off > self.len {
            return None;
        }
        // Avoid overflow: require `len <= self.len - off`.
        if len > self.len - off {
            return None;
        }
        Some(Self {
            inner: self.inner.clone(),
            off: self.off + off,
            len,
        })
    }

    /// Return a zero-copy sub-slice of this buffer.
    ///
    /// This never panics. If the range is invalid, it returns an empty slice.
    /// Prefer [`Bytes::try_slice`] when you need to distinguish errors.
    #[inline]
    pub fn slice(&self, off: usize, len: usize) -> Self {
        match self.try_slice(off, len) {
            Some(s) => s,
            None => {
                debug_assert!(
                    false,
                    "Bytes::slice out of bounds: off={off}, len={len}, self.len={}",
                    self.len
                );
                Self::empty()
            }
        }
    }

    /// Return a zero-copy slice from `off` to the end.
    ///
    /// This never panics. If `off > self.len`, it returns an empty slice.
    #[inline]
    pub fn slice_from(&self, off: usize) -> Self {
        if off > self.len {
            debug_assert!(
                false,
                "Bytes::slice_from out of bounds: off={off}, self.len={}",
                self.len
            );
            return Self::empty();
        }
        self.slice(off, self.len - off)
    }
}

impl From<Vec<u8>> for Bytes {
    #[inline]
    fn from(v: Vec<u8>) -> Self {
        let len = v.len();
        let arc = Arc::new(v);
        Self { inner: BytesInner::SharedVec(arc), off: 0, len }
    }
}

impl Deref for Bytes {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &Self::Target {
        match &self.inner {
            BytesInner::Static(s) => &s[self.off..self.off + self.len],
            BytesInner::SharedVec(v) => &v[self.off..self.off + self.len],
        }
    }
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::Bytes;
    use alloc::vec::Vec;

    #[test]
    fn from_vec_is_true_zero_copy_move() {
        // Use a larger capacity to catch any hidden "shrink" behavior.
        let mut v = Vec::with_capacity(128);
        v.extend_from_slice(b"hello world");
        let p = v.as_ptr();
        let len = v.len();

        let b = Bytes::from(v);
        assert_eq!(b.len(), len);
        assert_eq!(b.as_slice().as_ptr(), p);
        assert_eq!(b.as_slice(), b"hello world");
    }

    #[test]
    fn slice_is_zero_copy_and_pointer_offset_is_correct() {
        let mut v = Vec::with_capacity(64);
        v.extend_from_slice(b"0123456789");
        let p = v.as_ptr();

        let b = Bytes::from(v);
        let s = b.slice(3, 4);
        assert_eq!(s.as_slice(), b"3456");
        unsafe {
            assert_eq!(s.as_slice().as_ptr(), p.add(3));
        }
    }
}
