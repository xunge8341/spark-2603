use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Default)]
pub struct DataPlaneMetrics {
    pub accepted_total: AtomicU64,
    pub accept_errors_total: AtomicU64,
    pub accept_errors_transient_total: AtomicU64,
    pub accept_errors_fatal_total: AtomicU64,
    pub accept_rejected_total: AtomicU64,
    pub closed_total: AtomicU64,
    pub active_connections: AtomicU64,

    pub read_bytes_total: AtomicU64,
    pub write_bytes_total: AtomicU64,
    pub write_syscalls_total: AtomicU64,
    pub write_writev_calls_total: AtomicU64,

    pub backpressure_enter_total: AtomicU64,
    pub backpressure_exit_total: AtomicU64,

    pub draining_enter_total: AtomicU64,
    pub draining_exit_total: AtomicU64,
    pub draining_timeout_total: AtomicU64,

    pub decoded_msgs_total: AtomicU64,
    pub decode_errors_total: AtomicU64,
    pub inbound_frame_too_large_total: AtomicU64,

    pub flush_limited_total: AtomicU64,

    pub overload_reject_total: AtomicU64,
    pub overload_backpressure_total: AtomicU64,
    pub overload_close_total: AtomicU64,
    pub app_queue_high_watermark: AtomicU64,

    // Cumulation stats (copy/coalesce on stream read path).
    pub inbound_coalesce_total: AtomicU64,
    pub inbound_copied_bytes_total: AtomicU64,
    pub rx_lease_tokens_total: AtomicU64,
    pub rx_lease_borrowed_bytes_total: AtomicU64,
    pub rx_materialize_bytes_total: AtomicU64,
    pub rx_cumulation_copy_bytes_total: AtomicU64,

    // Driver scheduling kernel internals (stable counters, low overhead).
    pub driver_schedule_interest_sync_total: AtomicU64,
    pub driver_schedule_reclaim_total: AtomicU64,
    pub driver_schedule_draining_flush_total: AtomicU64,
    pub driver_schedule_write_kick_total: AtomicU64,
    pub driver_schedule_flush_followup_total: AtomicU64,
    pub driver_schedule_read_kick_total: AtomicU64,
    pub driver_schedule_stale_skipped_total: AtomicU64,
    // Reactor register de-dup internals.
    pub driver_interest_register_total: AtomicU64,
    pub driver_interest_register_skipped_total: AtomicU64,

    // Driver task ownership internals.
    pub driver_task_submit_total: AtomicU64,
    pub driver_task_submit_failed_total: AtomicU64,
    pub driver_task_submit_inflight_suppressed_total: AtomicU64,
    pub driver_task_submit_paused_suppressed_total: AtomicU64,
    pub driver_task_finish_total: AtomicU64,
    pub driver_task_reclaim_total: AtomicU64,
    pub driver_task_state_conflict_total: AtomicU64,
}

/// Instantaneous counter snapshot used by exporters, tests, and local perf scripts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DataPlaneMetricsSnapshot {
    pub accepted_total: u64,
    pub accept_errors_total: u64,
    pub accept_errors_transient_total: u64,
    pub accept_errors_fatal_total: u64,
    pub accept_rejected_total: u64,
    pub closed_total: u64,
    pub active_connections: u64,

    pub read_bytes_total: u64,
    pub write_bytes_total: u64,
    pub write_syscalls_total: u64,
    pub write_writev_calls_total: u64,

    pub backpressure_enter_total: u64,
    pub backpressure_exit_total: u64,

    pub draining_enter_total: u64,
    pub draining_exit_total: u64,
    pub draining_timeout_total: u64,

    pub decoded_msgs_total: u64,
    pub decode_errors_total: u64,
    pub inbound_frame_too_large_total: u64,

    pub flush_limited_total: u64,

    pub overload_reject_total: u64,
    pub overload_backpressure_total: u64,
    pub overload_close_total: u64,
    pub app_queue_high_watermark: u64,

    pub inbound_coalesce_total: u64,
    pub inbound_copied_bytes_total: u64,
    pub rx_lease_tokens_total: u64,
    pub rx_lease_borrowed_bytes_total: u64,
    pub rx_materialize_bytes_total: u64,
    pub rx_cumulation_copy_bytes_total: u64,

    pub driver_schedule_interest_sync_total: u64,
    pub driver_schedule_reclaim_total: u64,
    pub driver_schedule_draining_flush_total: u64,
    pub driver_schedule_write_kick_total: u64,
    pub driver_schedule_flush_followup_total: u64,
    pub driver_schedule_read_kick_total: u64,
    pub driver_schedule_stale_skipped_total: u64,
    pub driver_interest_register_total: u64,
    pub driver_interest_register_skipped_total: u64,

    pub driver_task_submit_total: u64,
    pub driver_task_submit_failed_total: u64,
    pub driver_task_submit_inflight_suppressed_total: u64,
    pub driver_task_submit_paused_suppressed_total: u64,
    pub driver_task_finish_total: u64,
    pub driver_task_reclaim_total: u64,
    pub driver_task_state_conflict_total: u64,
}

/// Derived dataplane cost indicators.
///
/// These are intentionally simple, zero-safe ratios that can be exported as gauges or printed by
/// local perf scripts:
/// - `write_syscalls_per_kib`: lower is usually better on the TX hot path;
/// - `write_writev_share_ratio`: higher means we are batching more writes;
/// - `inbound_copy_bytes_per_read_byte`: lower means less RX copy pressure;
/// - `inbound_coalesces_per_mib`: a coarse signal for stream cumulation churn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DataPlaneDerivedMetrics {
    pub write_syscalls_per_kib: f64,
    pub write_writev_share_ratio: f64,
    pub inbound_copy_bytes_per_read_byte: f64,
    pub inbound_coalesces_per_mib: f64,
}

impl Default for DataPlaneDerivedMetrics {
    fn default() -> Self {
        Self {
            write_syscalls_per_kib: 0.0,
            write_writev_share_ratio: 0.0,
            inbound_copy_bytes_per_read_byte: 0.0,
            inbound_coalesces_per_mib: 0.0,
        }
    }
}

impl DataPlaneMetrics {
    #[inline]
    pub fn snapshot(&self) -> DataPlaneMetricsSnapshot {
        DataPlaneMetricsSnapshot {
            accepted_total: self.accepted_total.load(Ordering::Relaxed),
            accept_errors_total: self.accept_errors_total.load(Ordering::Relaxed),
            accept_errors_transient_total: self
                .accept_errors_transient_total
                .load(Ordering::Relaxed),
            accept_errors_fatal_total: self.accept_errors_fatal_total.load(Ordering::Relaxed),
            accept_rejected_total: self.accept_rejected_total.load(Ordering::Relaxed),
            closed_total: self.closed_total.load(Ordering::Relaxed),
            active_connections: self.active_connections.load(Ordering::Relaxed),
            read_bytes_total: self.read_bytes_total.load(Ordering::Relaxed),
            write_bytes_total: self.write_bytes_total.load(Ordering::Relaxed),
            write_syscalls_total: self.write_syscalls_total.load(Ordering::Relaxed),
            write_writev_calls_total: self.write_writev_calls_total.load(Ordering::Relaxed),
            backpressure_enter_total: self.backpressure_enter_total.load(Ordering::Relaxed),
            backpressure_exit_total: self.backpressure_exit_total.load(Ordering::Relaxed),
            draining_enter_total: self.draining_enter_total.load(Ordering::Relaxed),
            draining_exit_total: self.draining_exit_total.load(Ordering::Relaxed),
            draining_timeout_total: self.draining_timeout_total.load(Ordering::Relaxed),
            decoded_msgs_total: self.decoded_msgs_total.load(Ordering::Relaxed),
            decode_errors_total: self.decode_errors_total.load(Ordering::Relaxed),
            inbound_frame_too_large_total: self
                .inbound_frame_too_large_total
                .load(Ordering::Relaxed),
            flush_limited_total: self.flush_limited_total.load(Ordering::Relaxed),
            overload_reject_total: self.overload_reject_total.load(Ordering::Relaxed),
            overload_backpressure_total: self.overload_backpressure_total.load(Ordering::Relaxed),
            overload_close_total: self.overload_close_total.load(Ordering::Relaxed),
            app_queue_high_watermark: self.app_queue_high_watermark.load(Ordering::Relaxed),
            inbound_coalesce_total: self.inbound_coalesce_total.load(Ordering::Relaxed),
            inbound_copied_bytes_total: self.inbound_copied_bytes_total.load(Ordering::Relaxed),
            rx_lease_tokens_total: self.rx_lease_tokens_total.load(Ordering::Relaxed),
            rx_lease_borrowed_bytes_total: self
                .rx_lease_borrowed_bytes_total
                .load(Ordering::Relaxed),
            rx_materialize_bytes_total: self.rx_materialize_bytes_total.load(Ordering::Relaxed),
            rx_cumulation_copy_bytes_total: self
                .rx_cumulation_copy_bytes_total
                .load(Ordering::Relaxed),

            driver_schedule_interest_sync_total: self
                .driver_schedule_interest_sync_total
                .load(Ordering::Relaxed),
            driver_schedule_reclaim_total: self
                .driver_schedule_reclaim_total
                .load(Ordering::Relaxed),
            driver_schedule_draining_flush_total: self
                .driver_schedule_draining_flush_total
                .load(Ordering::Relaxed),
            driver_schedule_write_kick_total: self
                .driver_schedule_write_kick_total
                .load(Ordering::Relaxed),
            driver_schedule_flush_followup_total: self
                .driver_schedule_flush_followup_total
                .load(Ordering::Relaxed),
            driver_schedule_read_kick_total: self
                .driver_schedule_read_kick_total
                .load(Ordering::Relaxed),
            driver_schedule_stale_skipped_total: self
                .driver_schedule_stale_skipped_total
                .load(Ordering::Relaxed),
            driver_interest_register_total: self
                .driver_interest_register_total
                .load(Ordering::Relaxed),
            driver_interest_register_skipped_total: self
                .driver_interest_register_skipped_total
                .load(Ordering::Relaxed),

            driver_task_submit_total: self.driver_task_submit_total.load(Ordering::Relaxed),
            driver_task_submit_failed_total: self
                .driver_task_submit_failed_total
                .load(Ordering::Relaxed),
            driver_task_submit_inflight_suppressed_total: self
                .driver_task_submit_inflight_suppressed_total
                .load(Ordering::Relaxed),
            driver_task_submit_paused_suppressed_total: self
                .driver_task_submit_paused_suppressed_total
                .load(Ordering::Relaxed),
            driver_task_finish_total: self.driver_task_finish_total.load(Ordering::Relaxed),
            driver_task_reclaim_total: self.driver_task_reclaim_total.load(Ordering::Relaxed),
            driver_task_state_conflict_total: self
                .driver_task_state_conflict_total
                .load(Ordering::Relaxed),
        }
    }

    #[inline]
    pub fn record_read(&self, bytes: usize) {
        if bytes > 0 {
            self.read_bytes_total
                .fetch_add(as_u64(bytes), Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn record_write(&self, bytes: usize, syscalls: u64, writev_calls: u64) {
        if bytes > 0 {
            self.write_bytes_total
                .fetch_add(as_u64(bytes), Ordering::Relaxed);
        }
        if syscalls > 0 {
            self.write_syscalls_total
                .fetch_add(syscalls, Ordering::Relaxed);
        }
        if writev_calls > 0 {
            self.write_writev_calls_total
                .fetch_add(writev_calls, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn record_decode(&self, decoded: usize, decode_errs: usize, too_large: u64) {
        if decoded > 0 {
            self.decoded_msgs_total
                .fetch_add(as_u64(decoded), Ordering::Relaxed);
        }
        if decode_errs > 0 {
            self.decode_errors_total
                .fetch_add(as_u64(decode_errs), Ordering::Relaxed);
        }
        if too_large > 0 {
            self.inbound_frame_too_large_total
                .fetch_add(too_large, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn record_inbound_cumulation(&self, coalesce_count: u64, copied_bytes: u64) {
        if coalesce_count > 0 {
            self.inbound_coalesce_total
                .fetch_add(coalesce_count, Ordering::Relaxed);
        }
        if copied_bytes > 0 {
            self.inbound_copied_bytes_total
                .fetch_add(copied_bytes, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn record_rx_cumulation_copy(&self, copied_bytes: u64) {
        if copied_bytes > 0 {
            self.rx_cumulation_copy_bytes_total
                .fetch_add(copied_bytes, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn record_rx_lease(&self, tokens: u64, borrowed_bytes: u64) {
        if tokens > 0 {
            self.rx_lease_tokens_total
                .fetch_add(tokens, Ordering::Relaxed);
        }
        if borrowed_bytes > 0 {
            self.rx_lease_borrowed_bytes_total
                .fetch_add(borrowed_bytes, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn record_rx_materialize(&self, bytes: u64) {
        if bytes > 0 {
            self.rx_materialize_bytes_total
                .fetch_add(bytes, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn record_flush_limited(&self) {
        self.flush_limited_total.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_overload_reject(&self, count: u64) {
        if count > 0 {
            self.overload_reject_total
                .fetch_add(count, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn record_overload_backpressure(&self, count: u64) {
        if count > 0 {
            self.overload_backpressure_total
                .fetch_add(count, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn record_overload_close(&self, count: u64) {
        if count > 0 {
            self.overload_close_total
                .fetch_add(count, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn observe_app_queue_high_watermark(&self, queue_len: u64) {
        let _ = self.app_queue_high_watermark.fetch_update(
            Ordering::Relaxed,
            Ordering::Relaxed,
            |cur| {
                if queue_len > cur {
                    Some(queue_len)
                } else {
                    None
                }
            },
        );
    }
}

impl DataPlaneMetricsSnapshot {
    #[inline]
    pub fn derive(&self) -> DataPlaneDerivedMetrics {
        DataPlaneDerivedMetrics {
            write_syscalls_per_kib: scaled_ratio(
                self.write_syscalls_total,
                1024,
                self.write_bytes_total,
            ),
            write_writev_share_ratio: ratio(
                self.write_writev_calls_total,
                self.write_syscalls_total,
            ),
            inbound_copy_bytes_per_read_byte: ratio(
                self.inbound_copied_bytes_total,
                self.read_bytes_total,
            ),
            inbound_coalesces_per_mib: scaled_ratio(
                self.inbound_coalesce_total,
                1024 * 1024,
                self.read_bytes_total,
            ),
        }
    }

    #[inline]
    pub fn saturating_delta_since(&self, base: &Self) -> Self {
        Self {
            accepted_total: self.accepted_total.saturating_sub(base.accepted_total),
            accept_errors_total: self
                .accept_errors_total
                .saturating_sub(base.accept_errors_total),
            accept_errors_transient_total: self
                .accept_errors_transient_total
                .saturating_sub(base.accept_errors_transient_total),
            accept_errors_fatal_total: self
                .accept_errors_fatal_total
                .saturating_sub(base.accept_errors_fatal_total),
            accept_rejected_total: self
                .accept_rejected_total
                .saturating_sub(base.accept_rejected_total),
            closed_total: self.closed_total.saturating_sub(base.closed_total),
            active_connections: self
                .active_connections
                .saturating_sub(base.active_connections),
            read_bytes_total: self.read_bytes_total.saturating_sub(base.read_bytes_total),
            write_bytes_total: self
                .write_bytes_total
                .saturating_sub(base.write_bytes_total),
            write_syscalls_total: self
                .write_syscalls_total
                .saturating_sub(base.write_syscalls_total),
            write_writev_calls_total: self
                .write_writev_calls_total
                .saturating_sub(base.write_writev_calls_total),
            backpressure_enter_total: self
                .backpressure_enter_total
                .saturating_sub(base.backpressure_enter_total),
            backpressure_exit_total: self
                .backpressure_exit_total
                .saturating_sub(base.backpressure_exit_total),
            draining_enter_total: self
                .draining_enter_total
                .saturating_sub(base.draining_enter_total),
            draining_exit_total: self
                .draining_exit_total
                .saturating_sub(base.draining_exit_total),
            draining_timeout_total: self
                .draining_timeout_total
                .saturating_sub(base.draining_timeout_total),
            decoded_msgs_total: self
                .decoded_msgs_total
                .saturating_sub(base.decoded_msgs_total),
            decode_errors_total: self
                .decode_errors_total
                .saturating_sub(base.decode_errors_total),
            inbound_frame_too_large_total: self
                .inbound_frame_too_large_total
                .saturating_sub(base.inbound_frame_too_large_total),
            flush_limited_total: self
                .flush_limited_total
                .saturating_sub(base.flush_limited_total),
            overload_reject_total: self
                .overload_reject_total
                .saturating_sub(base.overload_reject_total),
            overload_backpressure_total: self
                .overload_backpressure_total
                .saturating_sub(base.overload_backpressure_total),
            overload_close_total: self
                .overload_close_total
                .saturating_sub(base.overload_close_total),
            app_queue_high_watermark: self
                .app_queue_high_watermark
                .saturating_sub(base.app_queue_high_watermark),
            inbound_coalesce_total: self
                .inbound_coalesce_total
                .saturating_sub(base.inbound_coalesce_total),
            inbound_copied_bytes_total: self
                .inbound_copied_bytes_total
                .saturating_sub(base.inbound_copied_bytes_total),
            rx_lease_tokens_total: self
                .rx_lease_tokens_total
                .saturating_sub(base.rx_lease_tokens_total),
            rx_lease_borrowed_bytes_total: self
                .rx_lease_borrowed_bytes_total
                .saturating_sub(base.rx_lease_borrowed_bytes_total),
            rx_materialize_bytes_total: self
                .rx_materialize_bytes_total
                .saturating_sub(base.rx_materialize_bytes_total),
            rx_cumulation_copy_bytes_total: self
                .rx_cumulation_copy_bytes_total
                .saturating_sub(base.rx_cumulation_copy_bytes_total),

            driver_schedule_interest_sync_total: self
                .driver_schedule_interest_sync_total
                .saturating_sub(base.driver_schedule_interest_sync_total),
            driver_schedule_reclaim_total: self
                .driver_schedule_reclaim_total
                .saturating_sub(base.driver_schedule_reclaim_total),
            driver_schedule_draining_flush_total: self
                .driver_schedule_draining_flush_total
                .saturating_sub(base.driver_schedule_draining_flush_total),
            driver_schedule_write_kick_total: self
                .driver_schedule_write_kick_total
                .saturating_sub(base.driver_schedule_write_kick_total),
            driver_schedule_flush_followup_total: self
                .driver_schedule_flush_followup_total
                .saturating_sub(base.driver_schedule_flush_followup_total),
            driver_schedule_read_kick_total: self
                .driver_schedule_read_kick_total
                .saturating_sub(base.driver_schedule_read_kick_total),
            driver_schedule_stale_skipped_total: self
                .driver_schedule_stale_skipped_total
                .saturating_sub(base.driver_schedule_stale_skipped_total),
            driver_interest_register_total: self
                .driver_interest_register_total
                .saturating_sub(base.driver_interest_register_total),
            driver_interest_register_skipped_total: self
                .driver_interest_register_skipped_total
                .saturating_sub(base.driver_interest_register_skipped_total),

            driver_task_submit_total: self
                .driver_task_submit_total
                .saturating_sub(base.driver_task_submit_total),
            driver_task_submit_failed_total: self
                .driver_task_submit_failed_total
                .saturating_sub(base.driver_task_submit_failed_total),
            driver_task_submit_inflight_suppressed_total: self
                .driver_task_submit_inflight_suppressed_total
                .saturating_sub(base.driver_task_submit_inflight_suppressed_total),
            driver_task_submit_paused_suppressed_total: self
                .driver_task_submit_paused_suppressed_total
                .saturating_sub(base.driver_task_submit_paused_suppressed_total),
            driver_task_finish_total: self
                .driver_task_finish_total
                .saturating_sub(base.driver_task_finish_total),
            driver_task_reclaim_total: self
                .driver_task_reclaim_total
                .saturating_sub(base.driver_task_reclaim_total),
            driver_task_state_conflict_total: self
                .driver_task_state_conflict_total
                .saturating_sub(base.driver_task_state_conflict_total),
        }
    }
}

#[inline]
fn as_u64(v: usize) -> u64 {
    u64::try_from(v).unwrap_or(u64::MAX)
}

#[inline]
fn ratio(num: u64, den: u64) -> f64 {
    if den == 0 {
        0.0
    } else {
        num as f64 / den as f64
    }
}

#[inline]
fn scaled_ratio(num: u64, scale: u64, den: u64) -> f64 {
    if den == 0 {
        0.0
    } else {
        num.saturating_mul(scale) as f64 / den as f64
    }
}

// Exporters live in leaf crates (e.g. `spark-metrics-prometheus`).

#[cfg(test)]
mod tests {
    use super::{DataPlaneDerivedMetrics, DataPlaneMetrics, DataPlaneMetricsSnapshot};
    use std::sync::atomic::Ordering;

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn snapshot_reads_all_counters() {
        let m = DataPlaneMetrics::default();
        m.accepted_total.store(7, Ordering::Relaxed);
        m.write_bytes_total.store(2048, Ordering::Relaxed);
        m.write_syscalls_total.store(4, Ordering::Relaxed);
        m.write_writev_calls_total.store(2, Ordering::Relaxed);

        let snap = m.snapshot();
        assert_eq!(snap.accepted_total, 7);
        assert_eq!(snap.write_bytes_total, 2048);
        assert_eq!(snap.write_syscalls_total, 4);
        assert_eq!(snap.write_writev_calls_total, 2);
    }

    #[test]
    fn derive_is_zero_safe() {
        let d = DataPlaneMetricsSnapshot::default().derive();
        assert_eq!(d, DataPlaneDerivedMetrics::default());
    }

    #[test]
    fn derive_reports_expected_ratios() {
        let snap = DataPlaneMetricsSnapshot {
            read_bytes_total: 4096,
            write_bytes_total: 2048,
            write_syscalls_total: 4,
            write_writev_calls_total: 2,
            inbound_coalesce_total: 2,
            inbound_copied_bytes_total: 1024,
            ..DataPlaneMetricsSnapshot::default()
        };

        let d = snap.derive();
        assert_close(d.write_syscalls_per_kib, 2.0);
        assert_close(d.write_writev_share_ratio, 0.5);
        assert_close(d.inbound_copy_bytes_per_read_byte, 0.25);
        assert_close(d.inbound_coalesces_per_mib, 512.0);
    }

    #[test]
    fn helper_recorders_update_expected_counters() {
        let m = DataPlaneMetrics::default();
        m.record_read(4096);
        m.record_write(2048, 4, 2);
        m.record_decode(5, 1, 2);
        m.record_inbound_cumulation(3, 1024);
        m.record_rx_lease(2, 2048);
        m.record_rx_materialize(128);
        m.record_rx_cumulation_copy(4096);
        m.record_flush_limited();
        m.record_overload_reject(2);
        m.record_overload_backpressure(3);
        m.record_overload_close(1);
        m.observe_app_queue_high_watermark(6);
        m.observe_app_queue_high_watermark(4);

        let snap = m.snapshot();
        assert_eq!(snap.read_bytes_total, 4096);
        assert_eq!(snap.write_bytes_total, 2048);
        assert_eq!(snap.write_syscalls_total, 4);
        assert_eq!(snap.write_writev_calls_total, 2);
        assert_eq!(snap.decoded_msgs_total, 5);
        assert_eq!(snap.decode_errors_total, 1);
        assert_eq!(snap.inbound_frame_too_large_total, 2);
        assert_eq!(snap.inbound_coalesce_total, 3);
        assert_eq!(snap.inbound_copied_bytes_total, 1024);
        assert_eq!(snap.rx_lease_tokens_total, 2);
        assert_eq!(snap.rx_lease_borrowed_bytes_total, 2048);
        assert_eq!(snap.rx_materialize_bytes_total, 128);
        assert_eq!(snap.rx_cumulation_copy_bytes_total, 4096);
        assert_eq!(snap.flush_limited_total, 1);
        assert_eq!(snap.overload_reject_total, 2);
        assert_eq!(snap.overload_backpressure_total, 3);
        assert_eq!(snap.overload_close_total, 1);
        assert_eq!(snap.app_queue_high_watermark, 6);
    }

    #[test]
    fn delta_is_saturating_and_preserves_intervals() {
        let base = DataPlaneMetricsSnapshot {
            write_bytes_total: 1024,
            write_syscalls_total: 2,
            ..DataPlaneMetricsSnapshot::default()
        };
        let next = DataPlaneMetricsSnapshot {
            write_bytes_total: 4096,
            write_syscalls_total: 5,
            write_writev_calls_total: 3,
            ..DataPlaneMetricsSnapshot::default()
        };

        let delta = next.saturating_delta_since(&base);
        assert_eq!(delta.write_bytes_total, 3072);
        assert_eq!(delta.write_syscalls_total, 3);
        assert_eq!(delta.write_writev_calls_total, 3);

        let zeroed = base.saturating_delta_since(&next);
        assert_eq!(zeroed.write_bytes_total, 0);
        assert_eq!(zeroed.write_syscalls_total, 0);
    }
}
