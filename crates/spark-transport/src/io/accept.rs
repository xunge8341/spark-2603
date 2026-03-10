//! Accept error classification.
//!
//! Nonblocking accept has platform-specific behaviors (Windows/Unix) and can
//! surface transient errors during handshake. To keep backends consistent and
//! contract-testable, we define a small policy here.

use super::{classify_io_error, KernelError};

/// Decision for an error returned by `accept()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptDecision {
    /// Keep trying to accept more connections in this tick.
    Continue,
    /// Stop accepting for this tick (usually `WouldBlock`).
    Stop,
    /// Fatal error: stop accepting and surface a kernel-level error.
    Fatal(KernelError),
}

/// Classify a nonblocking `accept()` error.
///
/// Contract:
/// - `WouldBlock` => `Stop`.
/// - `Interrupted` => `Continue`.
/// - Aborted handshakes / resets during accept are transient => `Continue`.
/// - Anything else => `Fatal(classify_io_error(e))`.
#[inline]
pub fn classify_accept_error(e: &std::io::Error) -> AcceptDecision {
    use std::io::ErrorKind;

    match e.kind() {
        ErrorKind::WouldBlock => return AcceptDecision::Stop,
        ErrorKind::Interrupted => return AcceptDecision::Continue,

        // Common transient accept errors.
        ErrorKind::ConnectionAborted | ErrorKind::ConnectionReset => return AcceptDecision::Continue,

        _ => {}
    }

    #[cfg(windows)]
    if let Some(code) = e.raw_os_error() {
        // Best-effort minimal set. Keep contract-driven.
        match code {
            10035 => return AcceptDecision::Stop,      // WSAEWOULDBLOCK
            10053 | 10054 => return AcceptDecision::Continue, // WSAECONNABORTED / WSAECONNRESET
            _ => {}
        }
    }

    AcceptDecision::Fatal(classify_io_error(e))
}
