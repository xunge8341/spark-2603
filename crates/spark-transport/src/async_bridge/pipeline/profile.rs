/// Framing profile for the built-in stream decoder handler.
///
/// This keeps the hot-path core statically dispatched while still allowing
/// alternative framing strategies (e.g. HTTP/1 management plane) without forcing
/// users to understand the full pipeline internal types.
///
/// Notes on design:
/// - The profile is `Copy` so it can be stored in driver configs without heap allocation.
/// - For delimiter-based framing we support a bounded delimiter size (`MAX_DELIMITER_LEN`).
///   This covers common textual and binary protocols while keeping the profile POD.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameDecoderProfile {
    /// Line-based stream framing (bring-up default).
    Line { max_frame: usize },

    /// Delimiter-based stream framing.
    ///
    /// `consumed` always includes the delimiter bytes.
    /// `msg_end` optionally includes the delimiter bytes depending on `include_delimiter`.
    Delimiter {
        max_frame: usize,
        delimiter: DelimiterSpec,
        include_delimiter: bool,
    },

    /// Length-field prefixed stream framing.
    ///
    /// Wire format: `[len][payload]` where `len` is an unsigned integer encoded
    /// with the given `field_len` and `order`.
    ///
    /// Decoder strips the length field and emits only the payload bytes.
    LengthField {
        max_frame: usize,
        /// Width of the length field in bytes. Must be one of {1,2,3,4,8}.
        field_len: u8,
        order: spark_codec::ByteOrder,
    },

    /// Varint32 length-prefixed stream framing (protobuf-style).
    ///
    /// Wire format: `[varint32_len][payload]`.
    /// Decoder strips the varint prefix and emits only the payload bytes.
    Varint32 { max_frame: usize },

    /// HTTP/1 management-plane framing.
    ///
    /// The frame decoder emits **complete requests** (head + body bytes).
    ///
    /// Limits are enforced at the framing layer:
    /// - `max_request_bytes`: total (head + body) cap for a single request.
    /// - `max_head_bytes`: request line + headers cap (including CRLFs).
    /// - `max_headers`: header count cap.
    Http1 {
        max_request_bytes: usize,
        max_head_bytes: usize,
        max_headers: usize,
    },
}

/// Maximum supported delimiter length for [`FrameDecoderProfile::Delimiter`].
///
/// This is a pragmatic bound to keep the profile a small POD value and avoid heap allocation.
pub const MAX_DELIMITER_LEN: usize = 16;

/// A bounded delimiter specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DelimiterSpec {
    bytes: [u8; MAX_DELIMITER_LEN],
    len: u8,
}

impl DelimiterSpec {
    /// Build a delimiter spec from a byte slice.
    ///
    /// Returns `None` when the delimiter is empty or longer than [`MAX_DELIMITER_LEN`].
    pub fn new(delim: &[u8]) -> Option<Self> {
        if delim.is_empty() || delim.len() > MAX_DELIMITER_LEN {
            return None;
        }
        let mut bytes = [0u8; MAX_DELIMITER_LEN];
        bytes[..delim.len()].copy_from_slice(delim);
        Some(Self {
            bytes,
            len: delim.len() as u8,
        })
    }

    #[inline]
    pub fn len(self) -> usize {
        self.len as usize
    }

    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }
}

impl FrameDecoderProfile {
    #[inline]
    pub fn line(max_frame: usize) -> Self {
        Self::Line {
            max_frame: max_frame.max(1),
        }
    }

    /// Build a delimiter profile.
    ///
    /// Returns `None` when the delimiter is empty or too long.
    #[inline]
    pub fn delimiter(max_frame: usize, delimiter: &[u8], include_delimiter: bool) -> Option<Self> {
        let spec = DelimiterSpec::new(delimiter)?;
        Some(Self::Delimiter {
            max_frame: max_frame.max(1),
            delimiter: spec,
            include_delimiter,
        })
    }

    #[inline]
    pub fn http1(max_request_bytes: usize) -> Self {
        Self::http1_with_limits(max_request_bytes, max_request_bytes, 64)
    }

    /// Build an HTTP/1 framing profile with explicit head limits.
    #[inline]
    pub fn http1_with_limits(max_request_bytes: usize, max_head_bytes: usize, max_headers: usize) -> Self {
        let max_request_bytes = max_request_bytes.max(1);
        let max_head_bytes = max_head_bytes.max(1).min(max_request_bytes);
        let max_headers = max_headers.max(1);
        Self::Http1 {
            max_request_bytes,
            max_head_bytes,
            max_headers,
        }
    }

    /// Build a length-field prefixed framing profile.
    ///
    /// Returns `None` when `field_len` is unsupported.
    #[inline]
    pub fn length_field(max_frame: usize, field_len: usize, order: spark_codec::ByteOrder) -> Option<Self> {
        match field_len {
            1 | 2 | 3 | 4 | 8 => {}
            _ => return None,
        }
        Some(Self::LengthField {
            max_frame: max_frame.max(1),
            field_len: field_len as u8,
            order,
        })
    }

    /// Build a varint32 length-prefixed framing profile.
    #[inline]
    pub fn varint32(max_frame: usize) -> Self {
        Self::Varint32 {
            max_frame: max_frame.max(1),
        }
    }

    #[inline]
    pub fn max_frame_hint(self) -> usize {
        match self {
            Self::Line { max_frame } => max_frame,
            Self::Delimiter { max_frame, .. } => max_frame,
            Self::LengthField { max_frame, field_len, .. } => max_frame.saturating_add(field_len as usize),
            Self::Varint32 { max_frame } => max_frame.saturating_add(5),
            Self::Http1 { max_request_bytes, .. } => max_request_bytes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FrameDecoderProfile, MAX_DELIMITER_LEN};

    #[test]
    fn delimiter_profile_validates_bounds() {
        assert!(FrameDecoderProfile::delimiter(1024, b"\r\n", false).is_some());
        let too_long = vec![0u8; MAX_DELIMITER_LEN + 1];
        assert!(FrameDecoderProfile::delimiter(1024, &too_long, false).is_none());
        assert!(FrameDecoderProfile::delimiter(1024, b"", false).is_none());
    }
}
