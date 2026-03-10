use spark_transport::io::{classify_connect_error, ConnectDecision};

#[test]
fn connect_wouldblock_is_in_progress() {
    let e = std::io::Error::from(std::io::ErrorKind::WouldBlock);
    assert_eq!(classify_connect_error(&e), ConnectDecision::InProgress);
}

#[cfg(windows)]
#[test]
fn winsock_in_progress_codes_are_in_progress() {
    // WinSock: WSAEWOULDBLOCK/WSAEINPROGRESS/WSAEALREADY.
    for raw in [10035, 10036, 10037] {
        let e = std::io::Error::from_raw_os_error(raw);
        assert_eq!(classify_connect_error(&e), ConnectDecision::InProgress);
    }
}
