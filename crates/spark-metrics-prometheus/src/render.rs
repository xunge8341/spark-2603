//! Prometheus text-format renderer for dataplane metrics.

use spark_transport::DataPlaneMetrics;
use spark_uci::names::metrics as n;

const PREFIX: &str = "spark_dp_";

#[inline]
fn push_type_line(out: &mut String, name: &str, kind: &str) {
    out.push_str("# TYPE ");
    out.push_str(PREFIX);
    out.push_str(name);
    out.push(' ');
    out.push_str(kind);
    out.push('\n');
}

#[inline]
fn push_sample_u64(out: &mut String, name: &str, value: u64) {
    out.push_str(PREFIX);
    out.push_str(name);
    out.push(' ');
    out.push_str(&value.to_string());
    out.push('\n');
}

#[inline]
fn push_sample_f64(out: &mut String, name: &str, value: f64) {
    out.push_str(PREFIX);
    out.push_str(name);
    out.push(' ');
    out.push_str(&format!("{value:.6}"));
    out.push('\n');
}

/// Render dataplane counters to Prometheus text format.
///
/// Naming contract:
/// - counter/gauge suffix names are defined in `spark_uci::names::metrics`;
/// - exporters may prepend a subsystem prefix (here: `spark_dp_`).
///
/// Note: this is intentionally a standalone function (not a method on `DataPlaneMetrics`) so the
/// core crate stays exporter-free.
pub fn render_prometheus(m: &DataPlaneMetrics) -> String {
    let snap = m.snapshot();
    let derived = snap.derive();

    // Rough sizing: a few KB.
    let mut out = String::with_capacity(4096);

    // Counters / gauges.
    push_type_line(&mut out, n::ACCEPTED_TOTAL, "counter");
    push_sample_u64(&mut out, n::ACCEPTED_TOTAL, snap.accepted_total);

    push_type_line(&mut out, n::ACCEPT_ERRORS_TOTAL, "counter");
    push_sample_u64(&mut out, n::ACCEPT_ERRORS_TOTAL, snap.accept_errors_total);

    push_type_line(&mut out, n::ACCEPT_ERRORS_TRANSIENT_TOTAL, "counter");
    push_sample_u64(
        &mut out,
        n::ACCEPT_ERRORS_TRANSIENT_TOTAL,
        snap.accept_errors_transient_total,
    );

    push_type_line(&mut out, n::ACCEPT_ERRORS_FATAL_TOTAL, "counter");
    push_sample_u64(
        &mut out,
        n::ACCEPT_ERRORS_FATAL_TOTAL,
        snap.accept_errors_fatal_total,
    );

    push_type_line(&mut out, n::ACCEPT_REJECTED_TOTAL, "counter");
    push_sample_u64(
        &mut out,
        n::ACCEPT_REJECTED_TOTAL,
        snap.accept_rejected_total,
    );

    push_type_line(&mut out, n::CLOSED_TOTAL, "counter");
    push_sample_u64(&mut out, n::CLOSED_TOTAL, snap.closed_total);

    push_type_line(&mut out, n::ACTIVE_CONNECTIONS, "gauge");
    push_sample_u64(&mut out, n::ACTIVE_CONNECTIONS, snap.active_connections);

    push_type_line(&mut out, n::READ_BYTES_TOTAL, "counter");
    push_sample_u64(&mut out, n::READ_BYTES_TOTAL, snap.read_bytes_total);

    push_type_line(&mut out, n::WRITE_BYTES_TOTAL, "counter");
    push_sample_u64(&mut out, n::WRITE_BYTES_TOTAL, snap.write_bytes_total);

    push_type_line(&mut out, n::WRITE_SYSCALLS_TOTAL, "counter");
    push_sample_u64(&mut out, n::WRITE_SYSCALLS_TOTAL, snap.write_syscalls_total);

    push_type_line(&mut out, n::WRITE_WRITEV_CALLS_TOTAL, "counter");
    push_sample_u64(
        &mut out,
        n::WRITE_WRITEV_CALLS_TOTAL,
        snap.write_writev_calls_total,
    );

    push_type_line(&mut out, n::BACKPRESSURE_ENTER_TOTAL, "counter");
    push_sample_u64(
        &mut out,
        n::BACKPRESSURE_ENTER_TOTAL,
        snap.backpressure_enter_total,
    );

    push_type_line(&mut out, n::BACKPRESSURE_EXIT_TOTAL, "counter");
    push_sample_u64(
        &mut out,
        n::BACKPRESSURE_EXIT_TOTAL,
        snap.backpressure_exit_total,
    );

    push_type_line(&mut out, n::DECODED_MSGS_TOTAL, "counter");
    push_sample_u64(&mut out, n::DECODED_MSGS_TOTAL, snap.decoded_msgs_total);

    push_type_line(&mut out, n::DECODE_ERRORS_TOTAL, "counter");
    push_sample_u64(&mut out, n::DECODE_ERRORS_TOTAL, snap.decode_errors_total);

    push_type_line(&mut out, n::INBOUND_FRAME_TOO_LARGE_TOTAL, "counter");
    push_sample_u64(
        &mut out,
        n::INBOUND_FRAME_TOO_LARGE_TOTAL,
        snap.inbound_frame_too_large_total,
    );

    push_type_line(&mut out, n::FLUSH_LIMITED_TOTAL, "counter");
    push_sample_u64(&mut out, n::FLUSH_LIMITED_TOTAL, snap.flush_limited_total);

    push_type_line(&mut out, n::DRAINING_ENTER_TOTAL, "counter");
    push_sample_u64(&mut out, n::DRAINING_ENTER_TOTAL, snap.draining_enter_total);

    push_type_line(&mut out, n::DRAINING_EXIT_TOTAL, "counter");
    push_sample_u64(&mut out, n::DRAINING_EXIT_TOTAL, snap.draining_exit_total);

    push_type_line(&mut out, n::DRAINING_TIMEOUT_TOTAL, "counter");
    push_sample_u64(
        &mut out,
        n::DRAINING_TIMEOUT_TOTAL,
        snap.draining_timeout_total,
    );

    push_type_line(&mut out, n::INBOUND_COALESCE_TOTAL, "counter");
    push_sample_u64(
        &mut out,
        n::INBOUND_COALESCE_TOTAL,
        snap.inbound_coalesce_total,
    );

    push_type_line(&mut out, n::INBOUND_COPIED_BYTES_TOTAL, "counter");
    push_sample_u64(
        &mut out,
        n::INBOUND_COPIED_BYTES_TOTAL,
        snap.inbound_copied_bytes_total,
    );

    push_type_line(&mut out, n::RX_LEASE_TOKENS_TOTAL, "counter");
    push_sample_u64(
        &mut out,
        n::RX_LEASE_TOKENS_TOTAL,
        snap.rx_lease_tokens_total,
    );

    push_type_line(&mut out, n::RX_LEASE_BORROWED_BYTES_TOTAL, "counter");
    push_sample_u64(
        &mut out,
        n::RX_LEASE_BORROWED_BYTES_TOTAL,
        snap.rx_lease_borrowed_bytes_total,
    );

    push_type_line(&mut out, n::RX_MATERIALIZE_BYTES_TOTAL, "counter");
    push_sample_u64(
        &mut out,
        n::RX_MATERIALIZE_BYTES_TOTAL,
        snap.rx_materialize_bytes_total,
    );

    push_type_line(&mut out, n::RX_CUMULATION_COPY_BYTES_TOTAL, "counter");
    push_sample_u64(
        &mut out,
        n::RX_CUMULATION_COPY_BYTES_TOTAL,
        snap.rx_cumulation_copy_bytes_total,
    );

    // Driver scheduling kernel (internal counters).
    push_type_line(&mut out, n::DRIVER_SCHEDULE_INTEREST_SYNC_TOTAL, "counter");
    push_sample_u64(
        &mut out,
        n::DRIVER_SCHEDULE_INTEREST_SYNC_TOTAL,
        snap.driver_schedule_interest_sync_total,
    );

    push_type_line(&mut out, n::DRIVER_SCHEDULE_RECLAIM_TOTAL, "counter");
    push_sample_u64(
        &mut out,
        n::DRIVER_SCHEDULE_RECLAIM_TOTAL,
        snap.driver_schedule_reclaim_total,
    );

    push_type_line(&mut out, n::DRIVER_SCHEDULE_DRAINING_FLUSH_TOTAL, "counter");
    push_sample_u64(
        &mut out,
        n::DRIVER_SCHEDULE_DRAINING_FLUSH_TOTAL,
        snap.driver_schedule_draining_flush_total,
    );

    push_type_line(&mut out, n::DRIVER_SCHEDULE_WRITE_KICK_TOTAL, "counter");
    push_sample_u64(
        &mut out,
        n::DRIVER_SCHEDULE_WRITE_KICK_TOTAL,
        snap.driver_schedule_write_kick_total,
    );

    push_type_line(&mut out, n::DRIVER_SCHEDULE_FLUSH_FOLLOWUP_TOTAL, "counter");
    push_sample_u64(
        &mut out,
        n::DRIVER_SCHEDULE_FLUSH_FOLLOWUP_TOTAL,
        snap.driver_schedule_flush_followup_total,
    );

    push_type_line(&mut out, n::DRIVER_SCHEDULE_READ_KICK_TOTAL, "counter");
    push_sample_u64(
        &mut out,
        n::DRIVER_SCHEDULE_READ_KICK_TOTAL,
        snap.driver_schedule_read_kick_total,
    );

    push_type_line(&mut out, n::DRIVER_SCHEDULE_STALE_SKIPPED_TOTAL, "counter");
    push_sample_u64(
        &mut out,
        n::DRIVER_SCHEDULE_STALE_SKIPPED_TOTAL,
        snap.driver_schedule_stale_skipped_total,
    );

    // Reactor register de-dup (internal counters).
    push_type_line(&mut out, n::DRIVER_INTEREST_REGISTER_TOTAL, "counter");
    push_sample_u64(
        &mut out,
        n::DRIVER_INTEREST_REGISTER_TOTAL,
        snap.driver_interest_register_total,
    );

    push_type_line(
        &mut out,
        n::DRIVER_INTEREST_REGISTER_SKIPPED_TOTAL,
        "counter",
    );
    push_sample_u64(
        &mut out,
        n::DRIVER_INTEREST_REGISTER_SKIPPED_TOTAL,
        snap.driver_interest_register_skipped_total,
    );

    // Driver task ownership (internal counters).
    push_type_line(&mut out, n::DRIVER_TASK_SUBMIT_TOTAL, "counter");
    push_sample_u64(
        &mut out,
        n::DRIVER_TASK_SUBMIT_TOTAL,
        snap.driver_task_submit_total,
    );

    push_type_line(&mut out, n::DRIVER_TASK_SUBMIT_FAILED_TOTAL, "counter");
    push_sample_u64(
        &mut out,
        n::DRIVER_TASK_SUBMIT_FAILED_TOTAL,
        snap.driver_task_submit_failed_total,
    );

    push_type_line(
        &mut out,
        n::DRIVER_TASK_SUBMIT_INFLIGHT_SUPPRESSED_TOTAL,
        "counter",
    );
    push_sample_u64(
        &mut out,
        n::DRIVER_TASK_SUBMIT_INFLIGHT_SUPPRESSED_TOTAL,
        snap.driver_task_submit_inflight_suppressed_total,
    );

    push_type_line(
        &mut out,
        n::DRIVER_TASK_SUBMIT_PAUSED_SUPPRESSED_TOTAL,
        "counter",
    );
    push_sample_u64(
        &mut out,
        n::DRIVER_TASK_SUBMIT_PAUSED_SUPPRESSED_TOTAL,
        snap.driver_task_submit_paused_suppressed_total,
    );

    push_type_line(&mut out, n::DRIVER_TASK_FINISH_TOTAL, "counter");
    push_sample_u64(
        &mut out,
        n::DRIVER_TASK_FINISH_TOTAL,
        snap.driver_task_finish_total,
    );

    push_type_line(&mut out, n::DRIVER_TASK_RECLAIM_TOTAL, "counter");
    push_sample_u64(
        &mut out,
        n::DRIVER_TASK_RECLAIM_TOTAL,
        snap.driver_task_reclaim_total,
    );

    push_type_line(&mut out, n::DRIVER_TASK_STATE_CONFLICT_TOTAL, "counter");
    push_sample_u64(
        &mut out,
        n::DRIVER_TASK_STATE_CONFLICT_TOTAL,
        snap.driver_task_state_conflict_total,
    );

    // Derived gauges.
    push_type_line(&mut out, n::WRITE_SYSCALLS_PER_KIB, "gauge");
    push_sample_f64(
        &mut out,
        n::WRITE_SYSCALLS_PER_KIB,
        derived.write_syscalls_per_kib,
    );

    push_type_line(&mut out, n::WRITE_WRITEV_SHARE_RATIO, "gauge");
    push_sample_f64(
        &mut out,
        n::WRITE_WRITEV_SHARE_RATIO,
        derived.write_writev_share_ratio,
    );

    push_type_line(&mut out, n::INBOUND_COPY_BYTES_PER_READ_BYTE, "gauge");
    push_sample_f64(
        &mut out,
        n::INBOUND_COPY_BYTES_PER_READ_BYTE,
        derived.inbound_copy_bytes_per_read_byte,
    );

    push_type_line(&mut out, n::INBOUND_COALESCES_PER_MIB, "gauge");
    push_sample_f64(
        &mut out,
        n::INBOUND_COALESCES_PER_MIB,
        derived.inbound_coalesces_per_mib,
    );

    out
}

#[cfg(test)]
mod tests {
    use super::render_prometheus;
    use spark_transport::DataPlaneMetrics;
    use spark_uci::names::metrics as n;
    use std::sync::atomic::Ordering;

    fn full_name(name: &str) -> String {
        let mut s = String::with_capacity(super::PREFIX.len() + name.len());
        s.push_str(super::PREFIX);
        s.push_str(name);
        s
    }

    #[test]
    fn render_uses_stable_metric_names_contract() {
        let m = DataPlaneMetrics::default();
        m.accepted_total.store(1, Ordering::Relaxed);

        let text = render_prometheus(&m);
        let want = full_name(n::ACCEPTED_TOTAL);
        assert!(text.contains(&format!("# TYPE {want} counter")));
        assert!(text.contains(&format!("{want} 1")));
    }

    #[test]
    fn render_includes_derived_gauges_with_contract_names() {
        let m = DataPlaneMetrics::default();
        m.write_bytes_total.store(2048, Ordering::Relaxed);
        m.write_syscalls_total.store(4, Ordering::Relaxed);
        m.write_writev_calls_total.store(2, Ordering::Relaxed);
        m.read_bytes_total.store(4096, Ordering::Relaxed);
        m.inbound_copied_bytes_total.store(1024, Ordering::Relaxed);
        m.rx_lease_tokens_total.store(4, Ordering::Relaxed);
        m.rx_lease_borrowed_bytes_total
            .store(2048, Ordering::Relaxed);
        m.rx_materialize_bytes_total.store(128, Ordering::Relaxed);
        m.rx_cumulation_copy_bytes_total
            .store(1024, Ordering::Relaxed);
        m.inbound_coalesce_total.store(2, Ordering::Relaxed);

        let text = render_prometheus(&m);

        let syscalls_per_kib = full_name(n::WRITE_SYSCALLS_PER_KIB);
        assert!(text.contains(&format!("# TYPE {syscalls_per_kib} gauge")));
        assert!(text.contains(&format!("{syscalls_per_kib} 2.000000")));

        let writev_share = full_name(n::WRITE_WRITEV_SHARE_RATIO);
        assert!(text.contains(&format!("{writev_share} 0.500000")));

        let copy_ratio = full_name(n::INBOUND_COPY_BYTES_PER_READ_BYTE);
        assert!(text.contains(&format!("{copy_ratio} 0.250000")));

        let coalesce_per_mib = full_name(n::INBOUND_COALESCES_PER_MIB);
        assert!(text.contains(&format!("{coalesce_per_mib} 512.000000")));
    }

    #[test]
    fn render_includes_driver_kernel_internal_metrics() {
        let m = DataPlaneMetrics::default();
        m.driver_schedule_read_kick_total
            .store(7, Ordering::Relaxed);
        m.driver_task_submit_total.store(3, Ordering::Relaxed);
        m.driver_interest_register_total.store(5, Ordering::Relaxed);

        let text = render_prometheus(&m);

        let read_kick = full_name(n::DRIVER_SCHEDULE_READ_KICK_TOTAL);
        assert!(text.contains(&format!("# TYPE {read_kick} counter")));
        assert!(text.contains(&format!("{read_kick} 7")));

        let submit = full_name(n::DRIVER_TASK_SUBMIT_TOTAL);
        assert!(text.contains(&format!("{submit} 3")));

        let reg = full_name(n::DRIVER_INTEREST_REGISTER_TOTAL);
        assert!(text.contains(&format!("# TYPE {reg} counter")));
        assert!(text.contains(&format!("{reg} 5")));
    }
}
