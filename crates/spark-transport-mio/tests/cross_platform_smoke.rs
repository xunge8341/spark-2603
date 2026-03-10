use spark_buffer::Bytes;
use spark_core::context::Context;
use spark_core::service::Service;
use spark_transport::{DataPlaneConfig, DataPlaneMetrics, KernelError};

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct EchoPong;

impl Service<Bytes> for EchoPong {
    type Response = Option<Bytes>;
    type Error = KernelError;

    async fn call(&self, _context: Context, _request: Bytes) -> Result<Self::Response, Self::Error> {
        Ok(Some(Bytes::from_static(b"pong")))
    }
}

fn reserve_local_addr() -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.local_addr().expect("local addr")
}


fn read_exact_deadline(stream: &mut TcpStream, mut buf: &mut [u8], timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while !buf.is_empty() {
        match stream.read(buf) {
            Ok(0) => panic!("read_exact_deadline: EOF"),
            Ok(n) => {
                let tmp = buf;
                buf = &mut tmp[n..];
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut
                    || e.kind() == std::io::ErrorKind::Interrupted =>
            {
                if Instant::now() >= deadline {
                    panic!("read_exact_deadline: timeout: {e:?}");
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(e) => panic!("read_exact_deadline: {e:?}"),
        }
    }
}

#[test]
fn mio_tcp_smoke_multiple_clients() {
    let addr = reserve_local_addr();
    let draining = Arc::new(AtomicBool::new(false));
    let metrics = Arc::new(DataPlaneMetrics::default());

    let cfg = DataPlaneConfig::tcp(addr).with_drain_timeout(Duration::from_millis(250));
    let handle = spark_transport_mio::try_spawn_tcp_dataplane(
        cfg,
        Arc::clone(&draining),
        Arc::new(EchoPong),
        Arc::clone(&metrics),
    )
    .expect("spawn tcp dataplane");

    let concurrency = 8usize;
    let reqs_per_conn = 8usize;
    let mut threads = Vec::with_capacity(concurrency);
    for _ in 0..concurrency {
        threads.push(std::thread::spawn(move || {
            let mut stream = TcpStream::connect(addr).expect("connect");
            let _ = stream.set_nodelay(true);
            stream
                .set_read_timeout(Some(Duration::from_millis(200)))
                .expect("set read timeout");
            stream
                .set_write_timeout(Some(Duration::from_secs(1)))
                .expect("set write timeout");

            let mut buf = [0u8; 5];
            for _ in 0..reqs_per_conn {
                stream.write_all(b"ping\n").expect("write");
                read_exact_deadline(&mut stream, &mut buf, Duration::from_secs(2));
                assert_eq!(&buf, b"pong\n");
            }

            let _ = stream.shutdown(Shutdown::Both);
        }));
    }

    for t in threads {
        t.join().expect("join client");
    }

    draining.store(true, Ordering::Release);
    handle.join.join().expect("join");

    let snap = metrics.snapshot();
    assert!(snap.accepted_total >= 1);
    assert!(snap.read_bytes_total > 0);
    assert!(snap.write_bytes_total > 0);
}
