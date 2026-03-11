use std::time::Instant;

use spark_buffer::{Bytes, Cumulation};
use spark_transport::async_bridge::contract::{OutboundBuffer, WritabilityChange};
use spark_transport::async_bridge::OutboundFrame;
use spark_transport::policy::FlushBudget;
use spark_transport::DataPlaneMetrics;

use spark_transport_contract::fake_io::ScriptedIo;

fn env_usize(name: &str, default_value: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(default_value)
}

fn env_string(name: &str, default_value: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default_value.to_owned())
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
#[ignore = "manual perf smoke; run via scripts/perf_gate.sh"]
fn outbound_buffer_perf_smoke_reports_baseline() {
    let scenario = env_string("SPARK_PERF_SCENARIO", "small_packet_high_freq");
    let frames = env_usize("SPARK_PERF_FRAMES", 1024);
    let frame_bytes = env_usize("SPARK_PERF_FRAME_BYTES", 1024);
    let iterations = env_usize("SPARK_PERF_ITERATIONS", 64).max(1);
    let allowance_divisor = env_usize("SPARK_PERF_ALLOWANCE_DIVISOR", 1).max(1);

    let total_bytes = frames.saturating_mul(frame_bytes);
    let mut lat_us = Vec::with_capacity(iterations);
    let mut throughput_samples = Vec::with_capacity(iterations);
    let mut backpressure_events = 0u64;

    let metrics = DataPlaneMetrics::default();
    let base = metrics.snapshot();

    for _ in 0..iterations {
        let mut ob = OutboundBuffer::new(8 * 1024 * 1024, 4 * 1024 * 1024);
        for _ in 0..frames {
            let bytes = Bytes::from(vec![0u8; frame_bytes]);
            if let Ok(change) = ob.enqueue(OutboundFrame::from_bytes(bytes)) {
                if let WritabilityChange::BecameUnwritable { .. } = change {
                    backpressure_events = backpressure_events.saturating_add(1);
                }
            }
        }

        let mut io = ScriptedIo::new();
        io.add_allowance(total_bytes.saturating_mul(2) / allowance_divisor);

        let budget = FlushBudget::new(total_bytes.saturating_mul(2), frames.saturating_mul(2))
            .with_max_iov(16);

        let start = Instant::now();
        let (_, wrote, syscalls, writev_calls, _) = ob.flush_into(&mut io, budget);
        let elapsed = start.elapsed();
        metrics.record_write(wrote, syscalls, writev_calls);

        let micros = elapsed.as_micros() as u64;
        lat_us.push(micros);
        let secs = elapsed.as_secs_f64().max(f64::EPSILON);
        throughput_samples.push(wrote as f64 / secs);
    }

    let interval = metrics.snapshot().saturating_delta_since(&base);
    let derived = interval.derive();

    lat_us.sort_unstable();
    throughput_samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let p50 = percentile_us(&lat_us, 0.50);
    let p95 = percentile_us(&lat_us, 0.95);
    let p99 = percentile_us(&lat_us, 0.99);
    let throughput = if throughput_samples.is_empty() {
        0.0
    } else {
        let mid = throughput_samples.len() / 2;
        throughput_samples[mid]
    };

    let mut ob_ev = OutboundBuffer::new(8 * 1024 * 1024, 4 * 1024 * 1024);
    for _ in 0..frames {
        let _ = ob_ev.enqueue(OutboundFrame::from_bytes(Bytes::from(vec![
            0u8;
            frame_bytes
        ])));
    }
    let outbound_ev = ob_ev.alloc_evidence();

    let mut cum = Cumulation::with_capacity(1);
    for _ in 0..16 {
        cum.push_bytes(&[0u8; 1024]);
    }
    let cum_ev = cum.alloc_evidence();

    println!(
        "SPARK_PERF scenario={} tx_bytes={} iterations={} throughput_bytes_per_sec={:.3} p50_us={} p95_us={} p99_us={} syscalls={} writev_calls={} syscalls_per_kib={:.6} writev_share={:.6} copy_per_byte={:.6} backpressure_events={} ob_q_growth={} ob_peak_queue_len={} ob_peak_pending_bytes={} cum_tail_growth={} cum_tail_peak_capacity={}",
        scenario,
        interval.write_bytes_total,
        iterations,
        throughput,
        p50,
        p95,
        p99,
        interval.write_syscalls_total,
        interval.write_writev_calls_total,
        derived.write_syscalls_per_kib,
        derived.write_writev_share_ratio,
        derived.inbound_copy_bytes_per_read_byte,
        backpressure_events,
        outbound_ev.queue_capacity_growth_count,
        outbound_ev.peak_queue_len,
        outbound_ev.peak_pending_bytes,
        cum_ev.tail_capacity_growth_count,
        cum_ev.tail_peak_capacity,
    );

    assert!(!lat_us.is_empty());
    assert!(throughput > 0.0);
}
