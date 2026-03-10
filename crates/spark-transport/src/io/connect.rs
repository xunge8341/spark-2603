use super::classify_io_error;
use crate::KernelError;

/// Decision for nonblocking connect errors.
///
/// 说明：
/// - Rust `std::io::ErrorKind` 没有 EINPROGRESS/EALREADY 这类细分；
/// - Windows 上它们常以 raw error code 的形式出现（kind=Other）。
///
/// Contract:
/// - connect in progress => `InProgress`
/// - anything else => `Fatal(classify_io_error(e))`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectDecision {
    InProgress,
    Fatal(KernelError),
}

/// Classify connect errors for nonblocking sockets.
pub fn classify_connect_error(e: &std::io::Error) -> ConnectDecision {
    use std::io::ErrorKind;

    match e.kind() {
        ErrorKind::WouldBlock | ErrorKind::Interrupted => return ConnectDecision::InProgress,
        _ => {}
    }

    // Windows: WSAEWOULDBLOCK/WSAEINPROGRESS/WSAEALREADY are the common "in progress" trio.
    #[cfg(windows)]
    {
        if let Some(10035..=10037) = e.raw_os_error() {
            return ConnectDecision::InProgress;
        }
    }

    ConnectDecision::Fatal(classify_io_error(e))
}

#[cfg(test)]
mod tests {
    use super::{classify_connect_error, ConnectDecision};

    #[test]
    fn wouldblock_is_in_progress() {
        let e = std::io::Error::from(std::io::ErrorKind::WouldBlock);
        assert_eq!(classify_connect_error(&e), ConnectDecision::InProgress);
    }
}
