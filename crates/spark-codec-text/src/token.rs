#[inline]
pub fn is_token(bytes: &[u8]) -> bool {
    bytes.iter().all(|b| is_tchar(*b))
}

#[inline]
pub fn is_tchar(b: u8) -> bool {
    b.is_ascii_alphanumeric()
        || matches!(
            b,
            b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+' | b'-' | b'.' | b'^'
                | b'_' | b'`' | b'|' | b'~'
        )
}
