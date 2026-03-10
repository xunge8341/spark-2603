use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use mio::net::TcpStream as MioTcpStream;

use spark_transport::io::IoOps;
use spark_transport::KernelError;
use spark_transport_mio::TcpChannel;

/// Contract: TCP stream read(0) => KernelError::Eof.
#[test]
fn tcp_eof_is_classified_as_eof() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().unwrap();

    // Connect client and close immediately.
    let t = thread::spawn(move || {
        let s = TcpStream::connect(addr).expect("connect");
        drop(s);
    });

    let (srv, _) = listener.accept().expect("accept");
    srv.set_nonblocking(true).expect("nonblocking");
    let mio_srv = MioTcpStream::from_std(srv);

    let mut ch = TcpChannel::new(1, mio_srv, 1024).expect("tcp channel");

    let mut buf = [0u8; 16];
    let mut got_eof = false;

    for _ in 0..200 {
        match ch.try_read_into(&mut buf) {
            Err(KernelError::WouldBlock) | Err(KernelError::Interrupted) => {
                thread::sleep(Duration::from_millis(1));
                continue;
            }
            Err(KernelError::Eof) => {
                got_eof = true;
                break;
            }
            Err(other) => panic!("unexpected error: {:?}", other),
            Ok(_) => {}
        }
    }

    t.join().unwrap();
    assert!(got_eof, "expected Eof classification");
}
