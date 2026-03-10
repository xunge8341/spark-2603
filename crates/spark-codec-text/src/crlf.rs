/// Take a CRLF-delimited line starting at `start`.
///
/// Returns `(line_without_crlf, next_index_after_crlf)`.
#[inline]
pub fn take_crlf_line(buf: &[u8], start: usize) -> Option<(&[u8], usize)> {
    if start >= buf.len() {
        return None;
    }
    let mut i = start;
    while i + 1 < buf.len() {
        if buf[i] == b'\r' && buf[i + 1] == b'\n' {
            return Some((&buf[start..i], i + 2));
        }
        i += 1;
    }
    None
}
