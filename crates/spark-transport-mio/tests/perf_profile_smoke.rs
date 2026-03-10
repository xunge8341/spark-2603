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
        Ok(Some(Bytes::from_static(b"pong")))
    }
}

fn reserve_local_addr() -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.local_addr().expect("local addr")
}

#[test]
fn tcp_perf_profile_smoke_accepts_and_tracks_metrics() {
    let addr = reserve_local_addr();
    let draining = Arc::new(AtomicBool::new(false));
    let metrics = Arc::new(DataPlaneMetrics::default());

    let cfg = DataPlaneConfig::perf_tcp(addr).with_drain_timeout(Duration::from_millis(250));
    let handle = spark_transport_mio::try_spawn_tcp_dataplane(
        cfg.clone(),
        Arc::clone(&draining),
        Arc::new(EchoPong),
        Arc::clone(&metrics),
    )
    .expect("spawn perf dataplane");

    let mut stream = TcpStream::connect(addr).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .expect("set read timeout");
    stream.write_all(b"ping\n").expect("write");

    let mut buf = [0u8; 5];
    stream.read_exact(&mut buf).expect("read pong");
    assert_eq!(&buf, b"pong\n");

    let _ = stream.shutdown(Shutdown::Both);
    drop(stream);

    draining.store(true, Ordering::Release);
    handle.join.join().expect("join");

    let snap = metrics.snapshot();
    assert!(snap.accepted_total >= 1, "expected at least one accepted connection");
    assert!(snap.read_bytes_total >= 5, "expected request bytes to be observed");
    assert!(snap.write_bytes_total >= 5, "expected response bytes to be observed");
    assert!(snap.write_syscalls_total >= 1, "expected at least one write syscall");
    assert_eq!(cfg.flush_policy.max_syscalls, 64);
    assert_eq!(cfg.watermark.low_mul, 8);
    assert_eq!(cfg.watermark.high_mul, 16);
}
