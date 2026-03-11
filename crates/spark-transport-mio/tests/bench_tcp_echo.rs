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

    async fn call(
        &self,
        _context: Context,
        _request: Bytes,
    ) -> Result<Self::Response, Self::Error> {
        Ok(Some(Bytes::from_static(b"pong")))
    }
}

fn env_u64(name: &str, default_value: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(default_value)
}

fn env_string(name: &str, default_value: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default_value.to_owned())
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
    let scenario = env_string("SPARK_BENCH_SCENARIO", "tcp_echo_default");
    let concurrency = env_usize("SPARK_BENCH_CONCURRENCY", 32);
    let reqs_per_conn = env_usize("SPARK_BENCH_REQS_PER_CONN", 256);
    let request_bytes = env_usize("SPARK_BENCH_REQUEST_BYTES", 5).max(1);
    let expected_response = b"pong\n";
    let read_delay_us = env_u64("SPARK_BENCH_CLIENT_READ_DELAY_US", 0);
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
            let mut stream = match TcpStream::connect(addr) {
                Ok(stream) => stream,
                Err(_) => return (Vec::new(), reqs_per_conn),
            };
            let _ = stream.set_nodelay(true);
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));

            let mut lat_us = Vec::with_capacity(reqs_per_conn);
            let mut failed = 0usize;
            let mut request = vec![b'x'; request_bytes];
            request[request_bytes - 1] = b'\n';
            let mut buf = [0u8; 5];
            for _ in 0..reqs_per_conn {
                let t0 = Instant::now();
                if stream.write_all(&request).is_err() {
                    failed = failed.saturating_add(1);
                    continue;
                }
                if read_delay_us > 0 {
                    std::thread::sleep(Duration::from_micros(read_delay_us));
                }
                if stream.read_exact(&mut buf).is_err() {
                    failed = failed.saturating_add(1);
                    continue;
                }
                if &buf != expected_response {
                    failed = failed.saturating_add(1);
                    continue;
                }
                let dt = t0.elapsed();
                lat_us.push(dt.as_micros() as u64);
            }

            let _ = stream.shutdown(Shutdown::Both);
            (lat_us, failed)
        }));
    }

    let mut all_lat_us = Vec::with_capacity(concurrency * reqs_per_conn);
    let mut failed_requests = 0usize;
    for t in threads {
        if let Ok((lat, failed)) = t.join() {
            all_lat_us.extend(lat);
            failed_requests = failed_requests.saturating_add(failed);
        } else {
            failed_requests = failed_requests.saturating_add(reqs_per_conn);
        }
    }
    let elapsed = start.elapsed();

    draining.store(true, Ordering::Release);
    handle.join.join().expect("join dataplane");

    all_lat_us.sort_unstable();
    let p50 = percentile_us(&all_lat_us, 0.50);
    let p95 = percentile_us(&all_lat_us, 0.95);
    let p99 = percentile_us(&all_lat_us, 0.99);

    let successful_reqs = all_lat_us.len();
    let rps = successful_reqs as f64 / elapsed.as_secs_f64();

    let snap = metrics.snapshot();
    let derived = snap.derive();
    let total_requests = concurrency * reqs_per_conn;

    println!(
        "SPARK_BENCH scenario={} name=tcp_echo concurrency={} reqs_per_conn={} total_reqs={} successful_reqs={} failed_reqs={} request_bytes={} read_delay_us={} elapsed_ms={:.3} rps={:.3} p50_us={} p95_us={} p99_us={} write_syscalls_per_kib={:.6} writev_share={:.6} inbound_copy_bytes_per_read_byte={:.6} backpressure_enter_total={} flush_limited_total={} accepted_total={} closed_total={}",
        scenario,
        concurrency,
        reqs_per_conn,
        total_requests,
        successful_reqs,
        failed_requests,
        request_bytes,
        read_delay_us,
        elapsed.as_secs_f64() * 1000.0,
        rps,
        p50,
        p95,
        p99,
        derived.write_syscalls_per_kib,
        derived.write_writev_share_ratio,
        derived.inbound_copy_bytes_per_read_byte,
        snap.backpressure_enter_total,
        snap.flush_limited_total,
        snap.accepted_total,
        snap.closed_total,
    );

    // Basic sanity: benchmark ran and produced at least one sample.
    assert!(successful_reqs > 0);
    assert!(rps > 0.0);
}
