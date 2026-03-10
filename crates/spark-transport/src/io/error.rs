//! Std IO error classification.
//!
//! Backends should translate `std::io::Error` into `KernelError` using a stable mapping.
//! This keeps contract tests and cross-platform semantics consistent.

use super::KernelError;

/// Classify a `std::io::Error` into a kernel-level error category.
///
/// Contract:
/// - `WouldBlock` and `Interrupted` must be preserved (non-blocking readiness semantics).
/// - Common connection lifecycle errors collapse into `Closed`/`Reset`/`Timeout`.
/// - Anything else becomes `Internal(code)` (backend-specific).
#[inline]
pub fn classify_io_error(e: &std::io::Error) -> KernelError {
    use std::io::ErrorKind;

    match e.kind() {
        ErrorKind::WouldBlock => return KernelError::WouldBlock,
        ErrorKind::Interrupted => return KernelError::Interrupted,
        ErrorKind::TimedOut => return KernelError::Timeout,
        ErrorKind::ConnectionReset => return KernelError::Reset,

        // Stream lifecycle collapse.
        ErrorKind::BrokenPipe
        | ErrorKind::NotConnected
        | ErrorKind::ConnectionAborted
        | ErrorKind::ConnectionRefused
        | ErrorKind::InvalidInput => return KernelError::Closed,

        _ => {}
    }

    // Some platforms surface socket errors as `Other` even when the raw code is known.
    #[cfg(windows)]
    if let Some(code) = e.raw_os_error() {
        // WinSock codes: https://learn.microsoft.com/windows/win32/winsock/windows-sockets-error-codes-2
        // Keep this list minimal and contract-driven.
        match code {
            10035 => return KernelError::WouldBlock, // WSAEWOULDBLOCK
            10054 => return KernelError::Reset,      // WSAECONNRESET
            10053 | 10057 | 10058 | 10061 => return KernelError::Closed, // aborted/notconn/shutdown/refused
            _ => {}
        }
    }

    if let Some(code) = e.raw_os_error() {
        #[cfg(unix)]
        {
            return KernelError::Internal(
                crate::uci::internal_code(crate::error_codes::SUBSYS_OS_ERRNO, code as u32),
            );
        }
        #[cfg(windows)]
        {
            return KernelError::Internal(
                crate::uci::internal_code(crate::error_codes::SUBSYS_OS_WIN32, code as u32),
            );
        }
    }

    KernelError::Internal(crate::error_codes::ERR_IO_UNKNOWN)
}
