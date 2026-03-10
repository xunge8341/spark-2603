use std::time::Instant;

use spark_buffer::Bytes;
use spark_transport::async_bridge::contract::{FlushStatus, OutboundBuffer};
use spark_transport::async_bridge::OutboundFrame;
use spark_transport::DataPlaneMetrics;
use spark_transport::policy::FlushBudget;

use spark_transport_contract::fake_io::ScriptedIo;

/// Manual perf smoke for the TX hot path.
///
/// This is not a statistically rigorous benchmark. Its job is to give the team a stable,
/// zero-dependency local baseline for:
/// - total bytes flushed;
/// - effective MiB/s on the synthetic ScriptedIo path;
/// - syscall-per-KiB and writev share.
#[test]
#[ignore = "manual perf smoke; run via scripts/perf_baseline.*"]
fn outbound_buffer_perf_smoke_reports_baseline() {
    const FRAMES: usize = 1024;
    const FRAME_BYTES: usize = 1024;

    let total_bytes = FRAMES * FRAME_BYTES;
    let mut ob = OutboundBuffer::new(8 * 1024 * 1024, 4 * 1024 * 1024);
    for _ in 0..FRAMES {
        let bytes = Bytes::from(vec![0u8; FRAME_BYTES]);
        ob.enqueue(OutboundFrame::from_bytes(bytes));
    }

    let mut io = ScriptedIo::new();
    io.add_allowance(total_bytes.saturating_mul(2));

    // DECISION: use `FlushBudget::new()` (non-exhaustive) to keep this test resilient as the budget evolves.
    let budget = FlushBudget::new(total_bytes.saturating_mul(2), FRAMES.saturating_mul(2)).with_max_iov(16);

    let metrics = DataPlaneMetrics::default();
    let base = metrics.snapshot();

    let start = Instant::now();
    let (status, wrote, syscalls, writev_calls, _) = ob.flush_into(&mut io, budget);
    let elapsed = start.elapsed();
    metrics.record_write(wrote, syscalls, writev_calls);

    let interval = metrics.snapshot().saturating_delta_since(&base);
    let derived = interval.derive();
    let seconds = elapsed.as_secs_f64().max(f64::EPSILON);
    let mib_per_sec = interval.write_bytes_total as f64 / (1024.0 * 1024.0) / seconds;

    println!(
        "SPARK_PERF tx_bytes={} elapsed_ms={:.3} mib_per_sec={:.3} syscalls={} writev_calls={} syscalls_per_kib={:.6} writev_share={:.6}",
        interval.write_bytes_total,
        elapsed.as_secs_f64() * 1000.0,
        mib_per_sec,
        interval.write_syscalls_total,
        interval.write_writev_calls_total,
        derived.write_syscalls_per_kib,
        derived.write_writev_share_ratio
    );

    assert_eq!(status, FlushStatus::Drained);
    assert_eq!(wrote, total_bytes);
    assert!(ob.is_empty());
    assert!(syscalls > 0);
}
