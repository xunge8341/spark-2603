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

fn env_usize(name: &str, default_value: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(default_value)
}

fn percentile_us(sorted_micros: &[u64], pct: f64) -> u64 {
    if sorted_micros.is_empty() {
        return 0;
    }
    let clamped = pct.clamp(0.0, 1.0);
    let idx = ((sorted_micros.len() as f64 - 1.0) * clamped).round() as usize;
    sorted_micros[idx]
}

#[test]
#[ignore = "manual micro-bench; run via scripts/bench_tcp_echo.*"]
fn bench_tcp_echo_reports_latency_and_metrics() {
    let concurrency = env_usize("SPARK_BENCH_CONCURRENCY", 32);
    let reqs_per_conn = env_usize("SPARK_BENCH_REQS_PER_CONN", 256);
    let addr = reserve_local_addr();

    let draining = Arc::new(AtomicBool::new(false));
    let metrics = Arc::new(DataPlaneMetrics::default());

    let cfg = DataPlaneConfig::perf_tcp(addr).with_drain_timeout(Duration::from_millis(500));
    let handle = spark_transport_mio::try_spawn_tcp_dataplane(
        cfg,
        Arc::clone(&draining),
        Arc::new(EchoPong),
        Arc::clone(&metrics),
    )
    .expect("spawn dataplane");

    let start = Instant::now();
    let mut threads = Vec::with_capacity(concurrency);
    for _ in 0..concurrency {
        threads.push(std::thread::spawn(move || {
            let mut stream = TcpStream::connect(addr).expect("connect");
            let _ = stream.set_nodelay(true);
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("set read timeout");
            stream
                .set_write_timeout(Some(Duration::from_secs(2)))
                .expect("set write timeout");

            let mut lat_us = Vec::with_capacity(reqs_per_conn);
            let mut buf = [0u8; 5];
            for _ in 0..reqs_per_conn {
                let t0 = Instant::now();
                stream.write_all(b"ping\n").expect("write");
                stream.read_exact(&mut buf).expect("read");
                if &buf != b"pong\n" {
                    panic!("unexpected reply: {buf:?}");
                }
                let dt = t0.elapsed();
                lat_us.push(dt.as_micros() as u64);
            }

            let _ = stream.shutdown(Shutdown::Both);
            lat_us
        }));
    }

    let mut all_lat_us = Vec::with_capacity(concurrency * reqs_per_conn);
    for t in threads {
        all_lat_us.extend(t.join().expect("join client"));
    }
    let elapsed = start.elapsed();

    draining.store(true, Ordering::Release);
    handle.join.join().expect("join dataplane");

    all_lat_us.sort_unstable();
    let p50 = percentile_us(&all_lat_us, 0.50);
    let p95 = percentile_us(&all_lat_us, 0.95);
    let p99 = percentile_us(&all_lat_us, 0.99);

    let total_reqs = (concurrency * reqs_per_conn) as f64;
    let rps = total_reqs / elapsed.as_secs_f64();

    let snap = metrics.snapshot();
    let derived = snap.derive();

    println!(
        "SPARK_BENCH name=tcp_echo concurrency={} reqs_per_conn={} total_reqs={} elapsed_ms={:.3} rps={:.3} p50_us={} p95_us={} p99_us={} write_syscalls_per_kib={:.6} writev_share={:.6} inbound_copy_bytes_per_read_byte={:.6}",
        concurrency,
        reqs_per_conn,
        (concurrency * reqs_per_conn),
        elapsed.as_secs_f64() * 1000.0,
        rps,
        p50,
        p95,
        p99,
        derived.write_syscalls_per_kib,
        derived.write_writev_share_ratio,
        derived.inbound_copy_bytes_per_read_byte
    );

    // Basic sanity: benchmark ran and produced at least one sample.
    assert!(!all_lat_us.is_empty());
    assert!(rps > 0.0);
}
