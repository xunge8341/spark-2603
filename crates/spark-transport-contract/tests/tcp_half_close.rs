use std::io::Read;
use std::net::{Shutdown, TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use mio::net::TcpStream as MioTcpStream;

use spark_transport::io::IoOps;
use spark_transport::KernelError;
use spark_transport_mio::TcpChannel;

/// Contract: stream read(0) => Eof, but server-side write can still succeed (half-close).
#[test]
fn tcp_eof_does_not_imply_write_closed() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().unwrap();

    let t = thread::spawn(move || {
        let mut c = TcpStream::connect(addr).expect("connect");
        c.shutdown(Shutdown::Write).expect("shutdown write");

        let mut b = [0u8; 1];
        c.set_read_timeout(Some(Duration::from_secs(2)))
            .expect("read timeout");
        c.read_exact(&mut b).expect("read response");
        b[0]
    });

    let (srv, _) = listener.accept().expect("accept");
    srv.set_nonblocking(true).expect("nonblocking");
    let mio_srv = MioTcpStream::from_std(srv);
    let mut ch = TcpChannel::new(1, mio_srv, 1024).expect("tcp channel");

    // Read until EOF.
    let mut buf = [0u8; 16];
    for _ in 0..400 {
        match ch.try_read_into(&mut buf) {
            Err(KernelError::WouldBlock) | Err(KernelError::Interrupted) => {
                thread::sleep(Duration::from_millis(1));
                continue;
            }
            Err(KernelError::Eof) => break,
            Err(other) => panic!("unexpected error: {:?}", other),
            Ok(_) => {}
        }
    }

    // After EOF, write should still be allowed.
    let mut wrote = false;
    for _ in 0..200 {
        match ch.try_write(b"Z") {
            Ok(n) if n > 0 => {
                wrote = true;
                break;
            }
            Err(KernelError::WouldBlock) | Err(KernelError::Interrupted) => {
                thread::sleep(Duration::from_millis(1));
                continue;
            }
            Err(other) => panic!("unexpected write error: {:?}", other),
            Ok(_) => {}
        }
    }
    assert!(wrote, "expected write to succeed after EOF");

    // Flush is best-effort.
    let _ = ch.flush();

    let got = t.join().unwrap();
    assert_eq!(got, b'Z');
}
