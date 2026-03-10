/// Find the end of the HTTP header section (`\r\n\r\n`).
#[inline]
pub fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|pos| pos + 4)
}
