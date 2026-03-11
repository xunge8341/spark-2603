#![cfg_attr(not(windows), allow(unused_imports, dead_code))]
use spark_buffer::Bytes;
use spark_core::context::Context;
use spark_core::service::Service;
use spark_transport::policy::WatermarkPolicy;
use spark_transport::{Budget, DataPlaneConfig, DataPlaneMetrics, KernelError};

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct EchoPong;

impl Service<Bytes> for EchoPong {
    type Response = Option<Bytes>;
    type Error = KernelError;

    async fn call(
        &self,
        _context: Context,
        _request: Bytes,
    ) -> Result<Self::Response, Self::Error> {
        Ok(Some(Bytes::from_static(b"pong\n")))
    }
}

struct FixedPayload {
    reply: Bytes,
}

impl FixedPayload {
    fn new(payload_len: usize) -> Self {
        let mut v = Vec::with_capacity(payload_len);
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        for _ in 0..payload_len {
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            let b = (x.wrapping_mul(0x2545_F491_4F6C_DD1D) & 0xFF) as u8;
            v.push(b);
        }
        Self {
            reply: Bytes::from(v),
        }
    }
}

impl Service<Bytes> for FixedPayload {
    type Response = Option<Bytes>;
    type Error = KernelError;

    async fn call(
        &self,
        _context: Context,
        _request: Bytes,
    ) -> Result<Self::Response, Self::Error> {
        Ok(Some(self.reply.clone()))
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

#[cfg(windows)]
#[test]
fn windows_iocp_forward_progress_smoke() {
    let addr = reserve_local_addr();
    let draining = Arc::new(AtomicBool::new(false));
    let metrics = Arc::new(DataPlaneMetrics::default());

    let cfg = DataPlaneConfig::tcp(addr)
        .with_budget(Budget {
            max_events: 512,
            max_nanos: 4_000_000,
        })
        .with_drain_timeout(Duration::from_millis(500));

    let handle = spark_transport_iocp::try_spawn_tcp_dataplane(
        cfg,
        Arc::clone(&draining),
        Arc::new(EchoPong),
        Arc::clone(&metrics),
    )
    .expect("spawn tcp dataplane (iocp wrapper)");

    let mut stream = TcpStream::connect(addr).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_millis(200)))
        .expect("set read timeout");
    stream
        .set_write_timeout(Some(Duration::from_secs(1)))
        .expect("set write timeout");

    let reqs = 128usize;
    let mut buf = [0u8; 5];
    for _ in 0..reqs {
        stream.write_all(b"ping\n").expect("write");
        read_exact_deadline(&mut stream, &mut buf, Duration::from_secs(2));
        assert_eq!(&buf, b"pong\n");
    }

    let _ = stream.shutdown(Shutdown::Both);
    draining.store(true, Ordering::Release);
    handle.join.join().expect("join");

    let snap = metrics.snapshot();
    assert!(snap.decoded_msgs_total >= reqs as u64);
    assert!(snap.write_bytes_total >= (reqs * buf.len()) as u64);
}

#[cfg(windows)]
#[test]
fn windows_iocp_half_close_write_path_remains_open() {
    let addr = reserve_local_addr();
    let draining = Arc::new(AtomicBool::new(false));
    let metrics = Arc::new(DataPlaneMetrics::default());

    let cfg = DataPlaneConfig::tcp(addr).with_drain_timeout(Duration::from_millis(250));
    let handle = spark_transport_iocp::try_spawn_tcp_dataplane(
        cfg,
        Arc::clone(&draining),
        Arc::new(EchoPong),
        Arc::clone(&metrics),
    )
    .expect("spawn tcp dataplane (iocp wrapper)");

    let mut stream = TcpStream::connect(addr).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .expect("set read timeout");
    stream
        .set_write_timeout(Some(Duration::from_secs(1)))
        .expect("set write timeout");

    stream.write_all(b"ping\n").expect("write");
    stream.shutdown(Shutdown::Write).expect("shutdown write");

    let mut buf = [0u8; 5];
    read_exact_deadline(&mut stream, &mut buf, Duration::from_secs(2));
    assert_eq!(&buf, b"pong\n");

    let _ = stream.shutdown(Shutdown::Both);
    draining.store(true, Ordering::Release);
    handle.join.join().expect("join");
}

#[cfg(windows)]
#[test]
fn windows_iocp_backpressure_drain_smoke() {
    let addr = reserve_local_addr();
    let draining = Arc::new(AtomicBool::new(false));
    let metrics = Arc::new(DataPlaneMetrics::default());

    let watermark = WatermarkPolicy {
        min_frame: 1024,
        low_mul: 1,
        high_mul: 2,
    };

    let cfg = DataPlaneConfig::tcp(addr)
        .with_watermark(watermark)
        .with_budget(Budget {
            max_events: 512,
            max_nanos: 4_000_000,
        })
        .with_drain_timeout(Duration::from_millis(750));

    let payload_len = 128 * 1024;
    let reqs = 32usize;
    let app = Arc::new(FixedPayload::new(payload_len));

    let handle = spark_transport_iocp::try_spawn_tcp_dataplane(
        cfg,
        Arc::clone(&draining),
        app,
        Arc::clone(&metrics),
    )
    .expect("spawn tcp dataplane (iocp wrapper)");

    let mut stream = TcpStream::connect(addr).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_millis(200)))
        .expect("set read timeout");
    stream
        .set_write_timeout(Some(Duration::from_secs(1)))
        .expect("set write timeout");

    for _ in 0..reqs {
        stream.write_all(b"ping\n").expect("write");
    }

    std::thread::sleep(Duration::from_millis(50));

    let expected = (payload_len + 1) * reqs;
    let mut got = 0usize;
    let start = Instant::now();
    let mut buf = vec![0u8; 64 * 1024];

    while got < expected && start.elapsed() < Duration::from_secs(15) {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => panic!("read failed: {e:?}"),
        }
    }

    assert_eq!(
        got, expected,
        "did not receive full reply stream in time (got={}, expected={})",
        got, expected
    );

    let _ = stream.shutdown(Shutdown::Both);
    draining.store(true, Ordering::Release);
    handle.join.join().expect("join");

    let snap = metrics.snapshot();
    assert!(
        snap.backpressure_enter_total >= 1,
        "backpressure never engaged"
    );
    assert!(
        snap.backpressure_exit_total >= 1,
        "backpressure never exited"
    );
}

#[cfg(windows)]
#[test]
fn windows_iocp_reactor_wake_consistency_smoke() {
    let addr = reserve_local_addr();
    let draining = Arc::new(AtomicBool::new(false));
    let metrics = Arc::new(DataPlaneMetrics::default());

    let cfg = DataPlaneConfig::tcp(addr)
        .with_budget(Budget {
            max_events: 128,
            max_nanos: 2_000_000,
        })
        .with_drain_timeout(Duration::from_millis(250));
    let handle = spark_transport_iocp::try_spawn_tcp_dataplane(
        cfg,
        Arc::clone(&draining),
        Arc::new(EchoPong),
        Arc::clone(&metrics),
    )
    .expect("spawn tcp dataplane (iocp wrapper)");

    let mut stream = TcpStream::connect(addr).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_millis(200)))
        .expect("set read timeout");
    stream
        .set_write_timeout(Some(Duration::from_secs(1)))
        .expect("set write timeout");

    let mut buf = [0u8; 5];
    for _ in 0..8 {
        stream.write_all(b"ping\n").expect("write");
        read_exact_deadline(&mut stream, &mut buf, Duration::from_secs(2));
    }

    std::thread::sleep(Duration::from_millis(120));

    for _ in 0..8 {
        stream.write_all(b"ping\n").expect("write");
        read_exact_deadline(&mut stream, &mut buf, Duration::from_secs(2));
    }

    let _ = stream.shutdown(Shutdown::Both);
    draining.store(true, Ordering::Release);
    handle.join.join().expect("join");

    let snap = metrics.snapshot();
    assert!(snap.decoded_msgs_total >= 16);
    assert!(snap.write_bytes_total >= 16 * 5);
}

#[cfg(not(windows))]
fn skip(reason: &str) {
    eprintln!("skip (windows-only): {reason}");
}

#[cfg(not(windows))]
#[test]
fn windows_iocp_forward_progress_smoke() {
    skip("IOCP wrapper progression gates require Windows kernel APIs");
}

#[cfg(not(windows))]
#[test]
fn windows_iocp_half_close_write_path_remains_open() {
    skip("IOCP half-close behavior gate is Windows-specific");
}

#[cfg(not(windows))]
#[test]
fn windows_iocp_backpressure_drain_smoke() {
    skip("IOCP backpressure drain gate is Windows-specific");
}

#[cfg(not(windows))]
#[test]
fn windows_iocp_reactor_wake_consistency_smoke() {
    skip("IOCP reactor wake consistency gate is Windows-specific");
}
