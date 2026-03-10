//! Slice-based buffer views.

use spark_core::buffer::{Buffer, BufferMut};

/// Immutable slice view.
pub struct SliceBuf<'a> {
    pub bytes: &'a [u8],
}

impl<'a> Buffer for SliceBuf<'a> {
    fn as_slice(&self) -> &[u8] {
        self.bytes
    }
}

/// Mutable slice view.
pub struct SliceBufMut<'a> {
    pub bytes: &'a mut [u8],
}

impl<'a> Buffer for SliceBufMut<'a> {
    fn as_slice(&self) -> &[u8] {
        self.bytes
    }
}

impl<'a> BufferMut for SliceBufMut<'a> {
    fn as_mut_slice(&mut self) -> &mut [u8] {
        self.bytes
    }
}
