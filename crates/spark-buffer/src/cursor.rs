//! Cursor-style readers/writers for byte slices.
//!
//! This is similar in spirit to Netty's `ByteBuf` read/write primitives, but keeps
//! the API small and `no_std`-friendly.

use crate::{endian, varint};

/// Read-only cursor over a byte slice.
#[derive(Debug, Clone, Copy)]
pub struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    #[inline]
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    #[inline]
    pub fn position(&self) -> usize {
        self.pos
    }

    #[inline]
    pub fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }

    #[inline]
    pub fn peek(&self, n: usize) -> Option<&'a [u8]> {
        self.buf.get(self.pos..self.pos + n)
    }

    #[inline]
    pub fn read_bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let out = self.peek(n)?;
        self.pos += n;
        Some(out)
    }

    #[inline]
    pub fn read_u8(&mut self) -> Option<u8> {
        let b = *self.buf.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }

    /// Read an unsigned LEB128 varint32.
    #[inline]
    pub fn read_varint_u32(&mut self) -> Option<u32> {
        let (v, n) = varint::decode_u32(&self.buf[self.pos..])?;
        self.pos += n;
        Some(v)
    }

    #[inline]
    pub fn read_u16_be(&mut self) -> Option<u16> {
        let v = endian::read_u16_be_at(self.buf, self.pos)?;
        self.pos += 2;
        Some(v)
    }

    #[inline]
    pub fn read_u16_le(&mut self) -> Option<u16> {
        let v = endian::read_u16_le_at(self.buf, self.pos)?;
        self.pos += 2;
        Some(v)
    }

    #[inline]
    pub fn read_u32_be(&mut self) -> Option<u32> {
        let v = endian::read_u32_be_at(self.buf, self.pos)?;
        self.pos += 4;
        Some(v)
    }

    #[inline]
    pub fn read_u32_le(&mut self) -> Option<u32> {
        let v = endian::read_u32_le_at(self.buf, self.pos)?;
        self.pos += 4;
        Some(v)
    }

    #[inline]
    pub fn read_u64_be(&mut self) -> Option<u64> {
        let v = endian::read_u64_be_at(self.buf, self.pos)?;
        self.pos += 8;
        Some(v)
    }

    #[inline]
    pub fn read_u64_le(&mut self) -> Option<u64> {
        let v = endian::read_u64_le_at(self.buf, self.pos)?;
        self.pos += 8;
        Some(v)
    }

    #[inline]
    pub fn advance(&mut self, n: usize) -> bool {
        if self.remaining() < n {
            return false;
        }
        self.pos += n;
        true
    }
}

/// Mutable cursor over a byte slice.
#[derive(Debug)]
pub struct CursorMut<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> CursorMut<'a> {
    #[inline]
    pub fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    #[inline]
    pub fn position(&self) -> usize {
        self.pos
    }

    #[inline]
    pub fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }

    #[inline]
    pub fn write_bytes(&mut self, bytes: &[u8]) -> bool {
        let n = bytes.len();
        let Some(dst) = self.buf.get_mut(self.pos..self.pos + n) else {
            return false;
        };
        dst.copy_from_slice(bytes);
        self.pos += n;
        true
    }

    #[inline]
    pub fn write_u8(&mut self, v: u8) -> bool {
        let Some(dst) = self.buf.get_mut(self.pos) else { return false };
        *dst = v;
        self.pos += 1;
        true
    }

    /// Write an unsigned LEB128 varint32.
    #[inline]
    pub fn write_varint_u32(&mut self, v: u32) -> bool {
        let mut tmp = [0u8; 5];
        let n = varint::encode_u32(v, &mut tmp);
        self.write_bytes(&tmp[..n])
    }

    #[inline]
    pub fn write_u16_be(&mut self, v: u16) -> bool {
        endian::write_u16_be_at(self.buf, self.pos, v).is_some().then(|| { self.pos += 2; }).is_some()
    }

    #[inline]
    pub fn write_u16_le(&mut self, v: u16) -> bool {
        endian::write_u16_le_at(self.buf, self.pos, v).is_some().then(|| { self.pos += 2; }).is_some()
    }

    #[inline]
    pub fn write_u32_be(&mut self, v: u32) -> bool {
        endian::write_u32_be_at(self.buf, self.pos, v).is_some().then(|| { self.pos += 4; }).is_some()
    }

    #[inline]
    pub fn write_u32_le(&mut self, v: u32) -> bool {
        endian::write_u32_le_at(self.buf, self.pos, v).is_some().then(|| { self.pos += 4; }).is_some()
    }

    #[inline]
    pub fn write_u64_be(&mut self, v: u64) -> bool {
        endian::write_u64_be_at(self.buf, self.pos, v).is_some().then(|| { self.pos += 8; }).is_some()
    }

    #[inline]
    pub fn write_u64_le(&mut self, v: u64) -> bool {
        if endian::write_u64_le_at(self.buf, self.pos, v).is_none() {
            return false;
        }
        self.pos += 8;
        true
    }

    #[inline]
    pub fn advance(&mut self, n: usize) -> bool {
        if self.remaining() < n {
            return false;
        }
        self.pos += n;
        true
    }
}
