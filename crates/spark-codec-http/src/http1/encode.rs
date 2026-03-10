use std::io::Write;

use super::types::HttpVersion;

/// Write a simple HTTP/1.1 response.
///
/// The response is intentionally minimal and always uses `Connection: close`.
pub fn write_response<W: Write>(
    w: &mut W,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let status_line = status_line(status);

    // Keep header rendering safe and predictable.
    // - Enforce ASCII only (HTTP/1.x header syntax is ASCII).
    // - Reject CR/LF to prevent header injection.
    let content_type = sanitize_header_value(content_type);

    // Header write is allocation-free.
    write!(
        w,
        "{}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        status_line,
        content_type,
        body.len(),
    )?;

    if !body.is_empty() {
        w.write_all(body)?;
    }
    Ok(())
}

#[inline]
fn sanitize_header_value(v: &str) -> &str {
    if !v.is_ascii() {
        return "application/octet-stream";
    }
    if v.as_bytes().iter().any(|b| *b == b'\r' || *b == b'\n') {
        return "application/octet-stream";
    }
    v
}

#[inline]
fn status_line(status: u16) -> &'static str {
    // We currently always emit HTTP/1.1 for Spark's internal adapter.
    // If we need to negotiate in the future, we can thread version through.
    let _ = HttpVersion::Http11; // keep type referenced for doc intent.

    match status {
        200 => "HTTP/1.1 200 OK",
        400 => "HTTP/1.1 400 Bad Request",
        404 => "HTTP/1.1 404 Not Found",
        413 => "HTTP/1.1 413 Payload Too Large",
        500 => "HTTP/1.1 500 Internal Server Error",
        503 => "HTTP/1.1 503 Service Unavailable",
        _ => "HTTP/1.1 500 Internal Server Error",
    }
}
