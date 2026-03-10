//! Segment-based byte queue.
//!
//! This is an internal building block for future zero-copy cumulation strategies.
//! The initial implementation is intentionally small and explicit.

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::vec::Vec;

use crate::Bytes;

const EMPTY: &[u8] = b"";

/// A FIFO queue of immutable byte segments.
#[derive(Debug, Default)]
pub struct ByteQueue {
    segs: VecDeque<Bytes>,
    len: usize,
}

impl ByteQueue {
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Iterate over segments as byte slices in FIFO order.
    #[inline]
    pub fn iter_segments(&self) -> impl Iterator<Item = &[u8]> {
        self.segs.iter().map(|b| b.as_ref())
    }

    pub fn push(&mut self, bytes: Bytes) {
        if bytes.is_empty() {
            return;
        }
        self.len = self.len.saturating_add(bytes.len());
        self.segs.push_back(bytes);
    }

    pub fn push_slice(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.push(Bytes::copy_from_slice(bytes));
    }

    #[inline]
    pub fn peek_contiguous(&self) -> Option<&[u8]> {
        self.segs.front().map(|b| b.as_ref())
    }

    /// Advance (drop) `n` bytes from the front of the queue.
    pub fn advance(&mut self, mut n: usize) {
        n = n.min(self.len);
        while n > 0 {
            let Some(front) = self.segs.pop_front() else {
                break;
            };
            let fl = front.len();
            if n >= fl {
                n -= fl;
                self.len -= fl;
                continue;
            }

            // Keep the remaining part of the front segment.
            let rem = match front.try_slice(n, fl - n) {
                Some(s) => s,
                None => Bytes::from_static(EMPTY),
            };
            self.len -= n;
            n = 0;
            if !rem.is_empty() {
                self.segs.push_front(rem);
            }
        }
    }

    /// Pop exactly `n` bytes from the queue.
    ///
    /// - If the bytes are contained in a single segment, returns a zero-copy slice.
    /// - Otherwise, it coalesces into a new `Bytes` allocation.
    pub fn pop_exact(&mut self, n: usize) -> Option<Bytes> {
        if n == 0 {
            return Some(Bytes::from_static(EMPTY));
        }
        if n > self.len {
            return None;
        }

        // Fast path: first segment contains the whole range.
        if let Some(front) = self.segs.front() {
            if front.len() >= n {
                // Need to update the queue.
                let front = self.segs.pop_front().unwrap_or_else(|| Bytes::from_static(EMPTY));
                if front.len() == n {
                    self.len -= n;
                    return Some(front);
                }

                let head = front.try_slice(0, n)?;
                let tail = front.try_slice(n, front.len() - n)?;
                self.len -= n;
                if !tail.is_empty() {
                    self.segs.push_front(tail);
                }
                return Some(head);
            }
        }

        // Slow path: coalesce multiple segments.
        let mut out = Vec::with_capacity(n);
        let mut remaining = n;
        while remaining > 0 {
            let Some(seg) = self.segs.pop_front() else {
                break;
            };
            let sl = seg.len();
            if sl <= remaining {
                out.extend_from_slice(seg.as_ref());
                remaining -= sl;
                self.len -= sl;
                continue;
            }

            // Split the segment.
            let head = seg.try_slice(0, remaining)?;
            let tail = seg.try_slice(remaining, sl - remaining)?;
            out.extend_from_slice(head.as_ref());
            self.len -= remaining;
            remaining = 0;
            if !tail.is_empty() {
                self.segs.push_front(tail);
            }
        }

        debug_assert_eq!(out.len(), n);
        Some(Bytes::from(out))
    }
}

#[cfg(test)]
mod tests {
    use super::ByteQueue;
    use crate::Bytes;

    #[test]
    fn pop_exact_single_segment_zero_copy() {
        let mut q = ByteQueue::new();
        q.push(Bytes::copy_from_slice(b"abcdef"));
        let b = q.pop_exact(3).unwrap();
        assert_eq!(b.as_ref(), b"abc");
        assert_eq!(q.len(), 3);
        assert_eq!(q.peek_contiguous().unwrap(), b"def");
    }

    #[test]
    fn pop_exact_multi_segment_coalesce() {
        let mut q = ByteQueue::new();
        q.push(Bytes::copy_from_slice(b"ab"));
        q.push(Bytes::copy_from_slice(b"cd"));
        q.push(Bytes::copy_from_slice(b"ef"));
        let b = q.pop_exact(5).unwrap();
        assert_eq!(b.as_ref(), b"abcde");
        assert_eq!(q.len(), 1);
        assert_eq!(q.peek_contiguous().unwrap(), b"f");
    }
}
