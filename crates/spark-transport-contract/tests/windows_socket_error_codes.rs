//! Windows socket error codes contract.
//!
//! Goal: freeze the mapping from WinSock raw error codes to `KernelError` categories.

#[cfg(windows)]
mod windows_only {
    use spark_transport::io::classify_io_error;
    use spark_transport::KernelError;

    #[test]
    fn winsock_raw_error_codes_are_classified() {
        // WSAEWOULDBLOCK (10035)
        let e = std::io::Error::from_raw_os_error(10035);
        assert_eq!(classify_io_error(&e), KernelError::WouldBlock);

        // WSAECONNRESET (10054)
        let e = std::io::Error::from_raw_os_error(10054);
        assert_eq!(classify_io_error(&e), KernelError::Reset);

        // WSAECONNABORTED (10053) / WSAENOTCONN (10057) / WSAESHUTDOWN (10058) / WSAECONNREFUSED (10061)
        for code in [10053, 10057, 10058, 10061] {
            let e = std::io::Error::from_raw_os_error(code);
            assert_eq!(classify_io_error(&e), KernelError::Closed, "code={}", code);
        }
    }
}
