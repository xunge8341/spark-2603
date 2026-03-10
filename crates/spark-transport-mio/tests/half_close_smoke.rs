use spark_buffer::Bytes;
use spark_core::context::Context;
use spark_core::service::Service;
use spark_transport::{DataPlaneConfig, DataPlaneMetrics, KernelError};

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

struct EchoPong;

impl Service<Bytes> for EchoPong {
    type Response = Option<Bytes>;
    type Error = KernelError;

    async fn call(&self, _context: Context, _request: Bytes) -> Result<Self::Response, Self::Error> {
        // Return a line-terminated payload so this half-close contract test does not rely on
        // the outbound framing encoder being present for the TCP dataplane profile.
        //
        // The default line encoder is idempotent (`\n` already present => no double-append).
        Ok(Some(Bytes::from_static(b"pong\n")))
    }
}

fn reserve_local_addr() -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.local_addr().expect("local addr")
}

/// Cross-platform integration contract:
/// - EOF on read (peer half-closes) must NOT imply write is closed;
/// - server must still be able to write a response.
#[test]
fn mio_tcp_half_close_does_not_prevent_writes() {
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

    let mut stream = TcpStream::connect(addr).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .expect("set read timeout");
    stream
        .set_write_timeout(Some(Duration::from_secs(1)))
        .expect("set write timeout");

    stream.write_all(b"ping\n").expect("write");
    // Half-close write side; server must still be able to respond.
    stream.shutdown(Shutdown::Write).expect("shutdown write");

    let mut buf = [0u8; 5];
    if let Err(e) = stream.read_exact(&mut buf) {
        // Print dataplane snapshot to aid debugging on flaky platforms.
        eprintln!("half_close_smoke: read pong failed: {e:?} snap={:?}", metrics.snapshot());
        panic!("read pong: {e:?}");
    }
    assert_eq!(&buf, b"pong\n");

    let _ = stream.shutdown(Shutdown::Both);

    draining.store(true, Ordering::Release);
    handle.join.join().expect("join");

    let snap = metrics.snapshot();
    assert!(snap.accepted_total >= 1);
    assert!(snap.write_bytes_total > 0);
}
