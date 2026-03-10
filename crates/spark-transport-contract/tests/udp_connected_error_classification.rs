use spark_transport::io::classify_connected_udp_recv_error;
use spark_transport::KernelError;

#[cfg(windows)]
#[test]
fn winsock_udp_connreset_is_transient() {
    // WinSock: WSAECONNRESET(10054) on connected UDP recv.
    let e = std::io::Error::from_raw_os_error(10054);
    assert_eq!(classify_connected_udp_recv_error(&e), KernelError::WouldBlock);
}

#[cfg(not(windows))]
#[test]
fn udp_connrefused_is_transient() {
    let e = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused");
    assert_eq!(classify_connected_udp_recv_error(&e), KernelError::WouldBlock);
}
