use spark_transport::io::{classify_accept_error, AcceptDecision};

#[test]
fn accept_wouldblock_stops() {
    let e = std::io::Error::new(std::io::ErrorKind::WouldBlock, "wb");
    assert_eq!(classify_accept_error(&e), AcceptDecision::Stop);
}

#[test]
fn accept_interrupted_continues() {
    let e = std::io::Error::new(std::io::ErrorKind::Interrupted, "intr");
    assert_eq!(classify_accept_error(&e), AcceptDecision::Continue);
}

#[test]
fn accept_aborted_and_reset_are_transient() {
    let e = std::io::Error::new(std::io::ErrorKind::ConnectionAborted, "aborted");
    assert_eq!(classify_accept_error(&e), AcceptDecision::Continue);

    let e = std::io::Error::new(std::io::ErrorKind::ConnectionReset, "reset");
    assert_eq!(classify_accept_error(&e), AcceptDecision::Continue);
}

#[test]
fn accept_other_is_fatal() {
    let e = std::io::Error::other("other");
    match classify_accept_error(&e) {
        AcceptDecision::Fatal(_) => {}
        v => panic!("expected Fatal, got: {:?}", v),
    }
}

#[cfg(windows)]
mod windows_only {
    use super::*;

    #[test]
    fn winsock_raw_error_codes_follow_contract() {
        // WSAEWOULDBLOCK
        let e = std::io::Error::from_raw_os_error(10035);
        assert_eq!(classify_accept_error(&e), AcceptDecision::Stop);

        // WSAECONNABORTED / WSAECONNRESET
        let e = std::io::Error::from_raw_os_error(10053);
        assert_eq!(classify_accept_error(&e), AcceptDecision::Continue);
        let e = std::io::Error::from_raw_os_error(10054);
        assert_eq!(classify_accept_error(&e), AcceptDecision::Continue);
    }
}
