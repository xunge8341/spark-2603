//! Minimal mutable byte buffer.
//!
//! This is a small, dependency-light alternative to `bytes::BytesMut`.
//!
//! Design goals:
//! - `no_std` + `alloc` friendly.
//! - Provide just enough functionality for Spark's transport/codec hot path.
//! - Keep the semantics explicit and hard to misuse.

use alloc::vec::Vec;

use crate::Bytes;

/// A growable, mutable byte buffer with an internal "read cursor".
///
/// The live region is `buf[start..]`.
#[derive(Debug, Default)]
pub struct BytesMut {
    buf: Vec<u8>,
    start: usize,
}

impl BytesMut {
    #[inline]
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            buf: Vec::with_capacity(cap),
            start: 0,
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.buf.len().saturating_sub(self.start)
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.buf.capacity()
    }

    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        &self.buf[self.start..]
    }

    #[inline]
    pub fn clear(&mut self) {
        self.buf.clear();
        self.start = 0;
    }

    /// Ensure the buffer can append at least `additional` bytes without reallocating.
    pub fn reserve(&mut self, additional: usize) {
        self.maybe_compact_for_append(additional);
        self.buf.reserve(additional);
    }

    /// Append bytes to the end of the live region.
    pub fn extend_from_slice(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.maybe_compact_for_append(bytes.len());
        self.buf.extend_from_slice(bytes);
    }

    /// Consume `n` bytes from the front of the live region.
    pub fn advance(&mut self, n: usize) {
        let n = n.min(self.len());
        self.start += n;
        self.maybe_compact();
    }

    /// Compact the live region to the beginning of the backing vector.
    pub fn compact(&mut self) {
        if self.start == 0 {
            return;
        }
        let live = self.len();
        if live == 0 {
            self.clear();
            return;
        }

        // Move live bytes to the front.
        self.buf.copy_within(self.start.., 0);
        self.buf.truncate(live);
        self.start = 0;
    }

    /// Extract a message from the buffer and advance by `consumed` bytes.
    ///
    /// `msg_end` is the length of the message bytes within the consumed prefix.
    ///
    /// This is tailored for Netty-style decoders where:
    /// - `message` is a `0..msg_end` range into the current cumulation,
    /// - `consumed` is the number of bytes to drop (may include delimiter/metadata).
    ///
    /// Performance notes:
    /// - Fast-path: when the consumed prefix covers the entire live region and `start == 0`,
    ///   this returns a zero-copy `Bytes` by moving the backing `Vec`.
    /// - Fallback: copies only the message bytes, then advances.
    pub fn take_message(&mut self, consumed: usize, msg_end: usize) -> Bytes {
        let live = self.len();
        let consumed = consumed.min(live);
        let msg_end = msg_end.min(consumed);

        // Zero-copy fast-path: take the entire live buffer.
        if self.start == 0 && consumed == live {
            let mut v = core::mem::take(&mut self.buf);
            self.start = 0;
            if msg_end < v.len() {
                v.truncate(msg_end);
            }
            return Bytes::from(v);
        }

        // Copy only the message bytes.
        let msg = Bytes::copy_from_slice(&self.as_slice()[..msg_end]);
        self.advance(consumed);
        msg
    }

    fn maybe_compact_for_append(&mut self, incoming: usize) {
        // If we're holding onto a large dead prefix, compact before growth.
        if self.start >= 4096 {
            let live = self.len();
            if live.saturating_mul(2) <= self.buf.len() {
                self.compact();
            }
        }

        // Also compact if we would otherwise reallocate with a huge dead prefix.
        if incoming > 0 {
            let live = self.len();
            let dead = self.start;
            if dead > 0 && live.saturating_add(incoming) > self.buf.capacity() {
                self.compact();
            }
        }
    }

    fn maybe_compact(&mut self) {
        if self.start == 0 {
            return;
        }

        // If everything was consumed, reset without shifting.
        if self.start >= self.buf.len() {
            self.clear();
            return;
        }

        // Compact when the dead prefix grows beyond half.
        if self.start.saturating_mul(2) >= self.buf.len() {
            self.compact();
        }
    }

    /// Convert the current live region into an immutable `Bytes` and clear the buffer.
    ///
    /// Performance notes:
    /// - Fast-path: when `start == 0`, this is a true zero-copy move of the backing `Vec<u8>`.
    /// - Fallback: if a dead prefix exists, it copies only the live bytes, then clears.
    ///
    /// This is intentionally explicit (not an implicit `Into<Bytes>`) to avoid accidental
    /// "consume-and-freeze" on hot paths where the caller should be using `take_message`.
    pub fn freeze(&mut self) -> Bytes {
        let live = self.len();
        if live == 0 {
            self.clear();
            return Bytes::from_static(b"");
        }

        if self.start == 0 {
            let v = core::mem::take(&mut self.buf);
            self.start = 0;
            return Bytes::from(v);
        }

        let b = Bytes::copy_from_slice(self.as_slice());
        self.clear();
        b
    }
}

#[cfg(test)]
mod tests {
    use super::BytesMut;

    #[test]
    fn take_message_fast_path_moves_vec() {
        let mut b = BytesMut::with_capacity(16);
        b.extend_from_slice(b"abc\r\n");
        let msg = b.take_message(5, 3);
        assert_eq!(msg.as_ref(), b"abc");
        assert!(b.is_empty());
    }

    #[test]
    fn take_message_fallback_copies_only_message() {
        let mut b = BytesMut::with_capacity(16);
        b.extend_from_slice(b"abc\r\nxyz");
        let msg = b.take_message(5, 3);
        assert_eq!(msg.as_ref(), b"abc");
        assert_eq!(b.as_slice(), b"xyz");
    }
}
