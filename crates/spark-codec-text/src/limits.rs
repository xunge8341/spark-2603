/// Limits for CRLF-delimited request/response heads.
///
/// This is used by HTTP-like protocols that delimit the start-line + header block with
/// `\r\n\r\n`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextHeadLimits {
    pub max_head_bytes: usize,
    pub max_headers: usize,
    pub max_line_bytes: usize,
}

impl TextHeadLimits {
    /// Create limits with sane minimums.
    #[inline]
    pub fn new(max_head_bytes: usize, max_headers: usize, max_line_bytes: usize) -> Self {
        Self {
            max_head_bytes: max_head_bytes.max(256),
            max_headers: max_headers.max(8),
            max_line_bytes: max_line_bytes.max(64),
        }
    }
}

impl Default for TextHeadLimits {
    #[inline]
    fn default() -> Self {
        Self::new(16 * 1024, 64, 8 * 1024)
    }
}
