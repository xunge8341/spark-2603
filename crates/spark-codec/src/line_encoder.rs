//! Line-ending encoder utilities.
//!
//! This module complements [`crate::LineDecoder`] with a symmetric, allocation-free
//! encoder primitive.
//!
//! Design goals:
//! - No allocation: caller provides the output buffer.
//! - Idempotent by default: avoids double-append when the input already ends with `\n`.
//! - Minimal surface area: advanced protocol stacks can build on top.

/// Errors produced by [`LineEncoder`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEncodeError {
    /// The provided output slice is too small.
    OutputTooSmall,
}

/// Allocation-free line-ending encoder.
///
/// Default behavior appends `\n` when missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LineEncoder {
    newline: u8,
}

impl LineEncoder {
    /// Build a LF (`\n`) line encoder.
    #[inline]
    pub const fn lf() -> Self {
        Self { newline: b'\n' }
    }

    /// Return the byte used as line terminator (defaults to `\n`).
    #[inline]
    pub const fn newline(self) -> u8 {
        self.newline
    }

    /// Compute the encoded length for `input`.
    ///
    /// If `input` already ends with the newline, the encoded length equals `input.len()`.
    #[inline]
    pub fn encoded_len(self, input: &[u8]) -> usize {
        if input.last().copied() == Some(self.newline) {
            input.len()
        } else {
            input.len().saturating_add(1)
        }
    }

    /// Encode `input` into `out`.
    ///
    /// Returns the number of bytes written.
    pub fn encode_into(self, out: &mut [u8], input: &[u8]) -> Result<usize, LineEncodeError> {
        let need = self.encoded_len(input);
        if out.len() < need {
            return Err(LineEncodeError::OutputTooSmall);
        }

        let n = input.len();
        out[..n].copy_from_slice(input);
        if need > n {
            out[n] = self.newline;
        }
        Ok(need)
    }
}

#[cfg(test)]
mod tests {
    use super::{LineEncodeError, LineEncoder};

    #[test]
    fn encodes_and_is_idempotent_for_lf() {
        let enc = LineEncoder::lf();

        let mut out = [0u8; 8];
        let n = enc.encode_into(&mut out, b"ping").unwrap();
        assert_eq!(&out[..n], b"ping\n");

        let mut out2 = [0u8; 8];
        let n2 = enc.encode_into(&mut out2, b"pong\n").unwrap();
        assert_eq!(&out2[..n2], b"pong\n");
    }

    #[test]
    fn rejects_small_output() {
        let enc = LineEncoder::lf();
        let mut out = [0u8; 4];
        let err = enc.encode_into(&mut out, b"ping").unwrap_err();
        assert_eq!(err, LineEncodeError::OutputTooSmall);
    }
}
