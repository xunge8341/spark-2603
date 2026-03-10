use super::classify_io_error;
use crate::KernelError;

/// Classify errors from `recv()` on a **connected UDP socket**.
///
/// 背景：
/// - Windows 上 connected UDP 在收到 ICMP Port Unreachable 后，`recv()` 常返回 WSAECONNRESET(10054)。
/// - 这并不意味着“连接被关闭”（UDP 无连接），更像一次可忽略的瞬态错误。
///
/// Contract:
/// - WouldBlock/Interrupted => 原样返回
/// - ConnectionReset/ConnectionRefused (and WinSock 10054) => `WouldBlock`（忽略并继续）
/// - anything else => `classify_io_error(e)`
pub fn classify_connected_udp_recv_error(e: &std::io::Error) -> KernelError {
    use std::io::ErrorKind;
    match e.kind() {
        ErrorKind::WouldBlock => return KernelError::WouldBlock,
        ErrorKind::Interrupted => return KernelError::Interrupted,
        ErrorKind::ConnectionReset | ErrorKind::ConnectionRefused => return KernelError::WouldBlock,
        _ => {}
    }

    #[cfg(windows)]
    {
        if e.raw_os_error() == Some(10054) {
            // WSAECONNRESET on UDP recv: treat as transient.
            return KernelError::WouldBlock;
        }
    }

    classify_io_error(e)
}

#[cfg(test)]
mod tests {
    use super::classify_connected_udp_recv_error;
    use crate::KernelError;

    #[test]
    fn connrefused_is_transient() {
        let e = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused");
        assert_eq!(classify_connected_udp_recv_error(&e), KernelError::WouldBlock);
    }
}
