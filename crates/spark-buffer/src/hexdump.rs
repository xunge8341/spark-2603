//! Hex dump helpers for diagnostics.

use core::fmt;

/// Wrapper that formats bytes as a compact hex string.
///
/// This avoids allocating a `String` and is suitable for logs/errors.
pub struct Hex<'a>(pub &'a [u8]);

impl<'a> fmt::Display for Hex<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, b) in self.0.iter().enumerate() {
            if i != 0 {
                write!(f, " ")?;
            }
            write!(f, "{:02x}", b)?;
        }
        Ok(())
    }
}

impl<'a> fmt::Debug for Hex<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hex({})", self)
    }
}
