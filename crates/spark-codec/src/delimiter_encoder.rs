//! Delimiter appending encoder utilities.
//!
//! This module complements [`crate::DelimiterBasedFrameDecoder`] with a symmetric,
//! allocation-free encoder primitive.
//!
//! Design goals:
//! - No allocation: caller provides the output buffer.
//! - Idempotent: avoids double-append when the input already ends with the delimiter.

/// Errors produced by [`DelimiterEncoder`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelimiterEncodeError {
    /// Delimiter must not be empty.
    EmptyDelimiter,
    /// The provided output slice is too small.
    OutputTooSmall,
}

/// Allocation-free delimiter encoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DelimiterEncoder<'a> {
    delim: &'a [u8],
}

impl<'a> DelimiterEncoder<'a> {
    /// Build a delimiter encoder.
    ///
    /// Returns an error when the delimiter is empty.
    #[inline]
    pub fn new(delim: &'a [u8]) -> Result<Self, DelimiterEncodeError> {
        if delim.is_empty() {
            return Err(DelimiterEncodeError::EmptyDelimiter);
        }
        Ok(Self { delim })
    }

    #[inline]
    pub fn delimiter(&self) -> &'a [u8] {
        self.delim
    }

    /// Compute the encoded length for `input`.
    ///
    /// If `input` already ends with the delimiter, the encoded length equals `input.len()`.
    #[inline]
    pub fn encoded_len(&self, input: &[u8]) -> usize {
        if input.ends_with(self.delim) {
            input.len()
        } else {
            input.len().saturating_add(self.delim.len())
        }
    }

    /// Encode `input` into `out`.
    ///
    /// Returns the number of bytes written.
    pub fn encode_into(&self, out: &mut [u8], input: &[u8]) -> Result<usize, DelimiterEncodeError> {
        let need = self.encoded_len(input);
        if out.len() < need {
            return Err(DelimiterEncodeError::OutputTooSmall);
        }

        let n = input.len();
        out[..n].copy_from_slice(input);
        if need > n {
            out[n..need].copy_from_slice(self.delim);
        }
        Ok(need)
    }
}

#[cfg(test)]
mod tests {
    use super::{DelimiterEncodeError, DelimiterEncoder};

    #[test]
    fn encodes_and_is_idempotent() {
        let enc = DelimiterEncoder::new(b"\r\n").unwrap();

        let mut out = [0u8; 16];
        let n = enc.encode_into(&mut out, b"ping").unwrap();
        assert_eq!(&out[..n], b"ping\r\n");

        let mut out2 = [0u8; 16];
        let n2 = enc.encode_into(&mut out2, b"pong\r\n").unwrap();
        assert_eq!(&out2[..n2], b"pong\r\n");
    }

    #[test]
    fn rejects_empty_delimiter() {
        let err = DelimiterEncoder::new(b"").unwrap_err();
        assert_eq!(err, DelimiterEncodeError::EmptyDelimiter);
    }

    #[test]
    fn rejects_small_output() {
        let enc = DelimiterEncoder::new(b"\r\n").unwrap();
        let mut out = [0u8; 4];
        let err = enc.encode_into(&mut out, b"ping").unwrap_err();
        assert_eq!(err, DelimiterEncodeError::OutputTooSmall);
    }
}
