//! Fast byte scanning helpers.
//!
//! These are small, dependency-free primitives used by transport/codec hot paths.
//! The API is intentionally tiny and `no_std` friendly.

/// Find the first occurrence of `needle` in `haystack`.
///
/// Returns the byte index within `haystack`.
#[inline]
pub fn find_byte(haystack: &[u8], needle: u8) -> Option<usize> {
    // Small inputs: `position` is fine and avoids unsafe.
    if haystack.len() < 16 {
        return haystack.iter().position(|&b| b == needle);
    }

    // 64-bit little-endian fast path.
    #[cfg(all(target_pointer_width = "64", target_endian = "little"))]
    {
        // Safety:
        // - We only read within `0..len`.
        // - We use `read_unaligned`, so no alignment requirements.
        // - The algorithm is endian-sensitive; guarded by `target_endian = "little"`.
        use core::ptr;

        const REPEAT: u64 = 0x0101_0101_0101_0101;
        const HI_BITS: u64 = 0x8080_8080_8080_8080;

        let len = haystack.len();
        let ptr = haystack.as_ptr();
        let needle64 = (needle as u64).wrapping_mul(REPEAT);

        let mut i = 0usize;
        while i + 8 <= len {
            let chunk = unsafe { ptr::read_unaligned(ptr.add(i) as *const u64) };
            let x = chunk ^ needle64;
            // Detect zero bytes in `x`.
            let m = x.wrapping_sub(REPEAT) & !x & HI_BITS;
            if m != 0 {
                let byte_index = (m.trailing_zeros() as usize) >> 3;
                return Some(i + byte_index);
            }
            i += 8;
        }

        // Tail.
        for (j, b) in haystack[i..].iter().enumerate() {
            if *b == needle {
                return Some(i + j);
            }
        }
        None
    }

    #[cfg(not(all(target_pointer_width = "64", target_endian = "little")))]
    {
        haystack.iter().position(|&b| b == needle)
    }
}

#[cfg(test)]
mod tests {
    use super::find_byte;

    #[test]
    fn find_byte_basic() {
        assert_eq!(find_byte(b"", b'\n'), None);
        assert_eq!(find_byte(b"abc", b'c'), Some(2));
        assert_eq!(find_byte(b"abc", b'x'), None);
    }

    #[test]
    fn find_byte_tail() {
        let mut v = alloc::vec![b'a'; 64];
        v[63] = b'\n';
        assert_eq!(find_byte(&v, b'\n'), Some(63));
    }
}
