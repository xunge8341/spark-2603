use spark_buffer::Bytes;
use spark_core::context::Context;
use spark_core::service::Service;
use spark_transport::{Budget, DataPlaneConfig, DataPlaneMetrics, KernelError};
use spark_transport::policy::WatermarkPolicy;

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct FixedPayload {
    reply: Bytes,
}

impl FixedPayload {
    fn new(payload_len: usize) -> Self {
        // Deterministic payload to avoid compressible all-zero patterns.
        let mut v = Vec::with_capacity(payload_len);
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        for _ in 0..payload_len {
            // xorshift64*
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            let b = (x.wrapping_mul(0x2545_F491_4F6C_DD1D) & 0xFF) as u8;
            v.push(b);
        }
        Self { reply: Bytes::from(v) }
    }
}

impl Service<Bytes> for FixedPayload {
    type Response = Option<Bytes>;
    type Error = KernelError;

    async fn call(&self, _context: Context, _request: Bytes) -> Result<Self::Response, Self::Error> {
        Ok(Some(self.reply.clone()))
    }
}

fn reserve_local_addr() -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.local_addr().expect("local addr")
}

/// Cross-platform integration contract:
/// - outbound backpressure must engage when the peer stops reading;
/// - once the peer resumes reads, the channel must eventually flush all queued replies.
#[cfg_attr(windows, ignore = "KI-001: Windows mio write-pressure forward-progress stall (see docs/KNOWN_ISSUES.md)")]
#[test]
fn mio_tcp_write_pressure_progresses_and_recovers() {
    let addr = reserve_local_addr();
    let draining = Arc::new(AtomicBool::new(false));
    let metrics = Arc::new(DataPlaneMetrics::default());

    // Make the backpressure watermarks small enough that a few replies trigger the state machine.
    let watermark = WatermarkPolicy {
        min_frame: 1024,
        low_mul: 1,
        high_mul: 2,
    };

    let cfg = DataPlaneConfig::tcp(addr)
        .with_watermark(watermark)
        .with_budget(Budget { max_events: 512, max_nanos: 4_000_000 })
        .with_drain_timeout(Duration::from_millis(750));

    let payload_len = 256 * 1024;
    let reqs = 48usize;
    let app = Arc::new(FixedPayload::new(payload_len));

    let handle = spark_transport_mio::try_spawn_tcp_dataplane(cfg, Arc::clone(&draining), app, Arc::clone(&metrics))
        .expect("spawn tcp dataplane");

    let mut stream = TcpStream::connect(addr).expect("connect");
    let _ = stream.set_nodelay(true);
    stream
        .set_read_timeout(Some(Duration::from_millis(200)))
        .expect("set read timeout");
    stream
        .set_write_timeout(Some(Duration::from_secs(1)))
        .expect("set write timeout");

    // Queue many requests without reading responses to induce outbound backlog.
    for _ in 0..reqs {
        stream.write_all(b"ping\n").expect("write");
    }

    // Give the server a brief head start to build backlog.
    std::thread::sleep(Duration::from_millis(50));

    let expected = (payload_len + 1) * reqs; // line encoder appends '\n'
    let mut got = 0usize;
    let start = Instant::now();
    let mut buf = vec![0u8; 64 * 1024];

    while got < expected && start.elapsed() < Duration::from_secs(15) {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {
                // Keep polling until we drain all replies or hit the global timeout.
            }
            Err(e) => panic!("read failed: {e:?}"),
        }
    }

    assert_eq!(got, expected, "did not receive full reply stream in time (got={}, expected={})", got, expected);

    let _ = stream.shutdown(Shutdown::Both);

    draining.store(true, Ordering::Release);
    handle.join.join().expect("join");

    let snap = metrics.snapshot();
    assert!(snap.accepted_total >= 1);
    assert!(snap.decoded_msgs_total >= reqs as u64);
    assert!(snap.write_bytes_total >= expected as u64);

    // The whole point of this test: we should observe backpressure engage and recover.
    assert!(snap.backpressure_enter_total >= 1, "backpressure never engaged");
    assert!(snap.backpressure_exit_total >= 1, "backpressure never exited");
}
