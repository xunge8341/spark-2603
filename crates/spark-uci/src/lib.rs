#![no_std]

/// Task token used by executors. 64-bit value expressed as (idx, gen).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskToken {
    pub idx: u32,
    pub gen: u32,
}

impl TaskToken {
    pub const NULL: Self = Self {
        idx: u32::MAX,
        gen: 0,
    };
}

/// Receive buffer token, stable cursor for zero-copy receive.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RxToken(pub u64);

/// Monotonic timestamp abstraction.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Instant(pub u64);

/// Poll/drive budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    pub max_events: u32,
    pub max_nanos: u64,
}

/// Kernel-level error (no business semantics).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelError {
    WouldBlock,
    Interrupted,
    /// Peer performed an orderly shutdown (stream read returned 0).
    Eof,
    /// Connection reset by peer.
    Reset,
    /// Operation timed out.
    Timeout,
    Closed,
    Invalid,
    NoMem,
    Unsupported,
    Internal(u32),
}

/// Construct a stable `KernelError::Internal` code.
///
/// Code layout (u32):
/// - High 8 bits: subsystem id
/// - Low 24 bits: subsystem-specific detail
///
/// This keeps errors compact while preserving enough structure for
/// cross-platform diagnostics and contract tests.
#[inline]
pub const fn internal_code(subsystem: u8, detail: u32) -> u32 {
    ((subsystem as u32) << 24) | (detail & 0x00FF_FFFF)
}

/// Extract the subsystem id from an internal error code.
#[inline]
pub const fn internal_subsystem(code: u32) -> u8 {
    (code >> 24) as u8
}

/// Extract the subsystem-specific detail from an internal error code.
#[inline]
pub const fn internal_detail(code: u32) -> u32 {
    code & 0x00FF_FFFF
}

/// Evidence event used by ops/contract tests.
///
/// 说明：
/// - 该结构体刻意保持 `no_std` 友好（仅使用基础类型与 `&'static str`）。
/// - 上层可将其导出到日志/Prometheus/OTel 等系统，作为“证据事件”。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvidenceEvent {
    /// 事件名：例如 BackpressureEnter/Exit, DrainingEnter/Exit/Timeout。
    pub name: &'static str,
    pub reason: &'static str,
    pub value: u64,
    pub unit: &'static str,
    pub unit_mapping: &'static str,
    pub scope: &'static str,
    pub scope_id: u64,
    pub channel_id: u32,
    /// draining/backpressure 相关扩展字段（缺省为 0）。
    pub pending_write_bytes: u64,
    pub inflight: u32,
}

pub type Result<T> = core::result::Result<T, KernelError>;

/// Stable names used across dataplane/mgmt/contract tests.
///
/// Rationale:
/// - Keep observability and contract suite semantics consistent across OS/backends.
/// - Avoid scattered string literals (typos become production incidents).
/// - Remain `no_std` friendly (`&'static str` only).
pub mod names {
    /// Evidence event names and schema version tags.
    pub mod evidence {
        // Event names (stable).
        pub const BACKPRESSURE_ENTER: &str = "BackpressureEnter";
        pub const BACKPRESSURE_EXIT: &str = "BackpressureExit";
        pub const DRAINING_ENTER: &str = "DrainingEnter";
        pub const DRAINING_EXIT: &str = "DrainingExit";
        pub const DRAINING_TIMEOUT: &str = "DrainingTimeout";
        pub const FLUSH_LIMITED: &str = "FlushLimited";
        pub const FRAME_TOO_LARGE: &str = "FrameTooLarge";
        pub const PEER_HALF_CLOSE: &str = "PeerHalfClose";
        pub const CLOSE_REQUESTED: &str = "CloseRequested";
        pub const CLOSE_COMPLETE: &str = "CloseComplete";
        pub const ABORTIVE_CLOSE: &str = "AbortiveClose";

        // Unit mappings (stable).
        pub const UNITMAP_BACKPRESSURE_BYTES_V1: &str = "bp_bytes_v1";
        pub const UNITMAP_DRAINING_V1: &str = "draining_v1";
        pub const UNITMAP_FLUSH_LIMITED_V1: &str = "flush_limited_v1";
        pub const UNITMAP_FRAME_TOO_LARGE_V1: &str = "frame_too_large_v1";
        pub const UNITMAP_CLOSE_V1: &str = "close_v1";
    }

    /// Metrics naming contract.
    ///
    /// Note: exporters may prefix these with a subsystem name (e.g. `spark_transport_*`).
    pub mod metrics {
        pub const ACCEPTED_TOTAL: &str = "accepted_total";
        pub const ACCEPT_ERRORS_TOTAL: &str = "accept_errors_total";
        pub const ACCEPT_ERRORS_TRANSIENT_TOTAL: &str = "accept_errors_transient_total";
        pub const ACCEPT_ERRORS_FATAL_TOTAL: &str = "accept_errors_fatal_total";
        pub const ACCEPT_REJECTED_TOTAL: &str = "accept_rejected_total";
        pub const CLOSED_TOTAL: &str = "closed_total";
        pub const ACTIVE_CONNECTIONS: &str = "active_connections";

        pub const READ_BYTES_TOTAL: &str = "read_bytes_total";
        pub const WRITE_BYTES_TOTAL: &str = "write_bytes_total";
        pub const WRITE_SYSCALLS_TOTAL: &str = "write_syscalls_total";
        pub const WRITE_WRITEV_CALLS_TOTAL: &str = "write_writev_calls_total";

        pub const BACKPRESSURE_ENTER_TOTAL: &str = "backpressure_enter_total";
        pub const BACKPRESSURE_EXIT_TOTAL: &str = "backpressure_exit_total";

        pub const DRAINING_ENTER_TOTAL: &str = "draining_enter_total";
        pub const DRAINING_EXIT_TOTAL: &str = "draining_exit_total";
        pub const DRAINING_TIMEOUT_TOTAL: &str = "draining_timeout_total";

        pub const DECODED_MSGS_TOTAL: &str = "decoded_msgs_total";
        pub const DECODE_ERRORS_TOTAL: &str = "decode_errors_total";
        pub const INBOUND_FRAME_TOO_LARGE_TOTAL: &str = "inbound_frame_too_large_total";
        pub const FLUSH_LIMITED_TOTAL: &str = "flush_limited_total";

        pub const OVERLOAD_REJECT_TOTAL: &str = "overload_reject_total";
        pub const OVERLOAD_BACKPRESSURE_TOTAL: &str = "overload_backpressure_total";
        pub const OVERLOAD_CLOSE_TOTAL: &str = "overload_close_total";
        pub const APP_QUEUE_HIGH_WATERMARK: &str = "app_queue_high_watermark";

        pub const INBOUND_COALESCE_TOTAL: &str = "inbound_coalesce_total";
        pub const INBOUND_COPIED_BYTES_TOTAL: &str = "inbound_copied_bytes_total";

        pub const RX_LEASE_TOKENS_TOTAL: &str = "rx_lease_tokens_total";
        pub const RX_LEASE_BORROWED_BYTES_TOTAL: &str = "rx_lease_borrowed_bytes_total";
        pub const RX_MATERIALIZE_BYTES_TOTAL: &str = "rx_materialize_bytes_total";
        pub const RX_CUMULATION_COPY_BYTES_TOTAL: &str = "rx_cumulation_copy_bytes_total";

        // Driver scheduling kernel (internal, stable counter names).
        //
        // These metrics help explain dataplane behavior without changing user-visible semantics.
        // They are intentionally label-free (suffix-only) to keep the naming contract simple.
        pub const DRIVER_SCHEDULE_INTEREST_SYNC_TOTAL: &str = "driver_schedule_interest_sync_total";
        pub const DRIVER_SCHEDULE_RECLAIM_TOTAL: &str = "driver_schedule_reclaim_total";
        pub const DRIVER_SCHEDULE_DRAINING_FLUSH_TOTAL: &str =
            "driver_schedule_draining_flush_total";
        pub const DRIVER_SCHEDULE_WRITE_KICK_TOTAL: &str = "driver_schedule_write_kick_total";
        pub const DRIVER_SCHEDULE_FLUSH_FOLLOWUP_TOTAL: &str =
            "driver_schedule_flush_followup_total";
        pub const DRIVER_SCHEDULE_READ_KICK_TOTAL: &str = "driver_schedule_read_kick_total";
        pub const DRIVER_SCHEDULE_STALE_SKIPPED_TOTAL: &str = "driver_schedule_stale_skipped_total";

        // Reactor register de-dup (internal, stable counter names).
        pub const DRIVER_INTEREST_REGISTER_TOTAL: &str = "driver_interest_register_total";
        pub const DRIVER_INTEREST_REGISTER_SKIPPED_TOTAL: &str =
            "driver_interest_register_skipped_total";

        // Driver task ownership (internal, stable counter names).
        pub const DRIVER_TASK_SUBMIT_TOTAL: &str = "driver_task_submit_total";
        pub const DRIVER_TASK_SUBMIT_FAILED_TOTAL: &str = "driver_task_submit_failed_total";
        pub const DRIVER_TASK_SUBMIT_INFLIGHT_SUPPRESSED_TOTAL: &str =
            "driver_task_submit_inflight_suppressed_total";
        pub const DRIVER_TASK_SUBMIT_PAUSED_SUPPRESSED_TOTAL: &str =
            "driver_task_submit_paused_suppressed_total";
        pub const DRIVER_TASK_FINISH_TOTAL: &str = "driver_task_finish_total";
        pub const DRIVER_TASK_RECLAIM_TOTAL: &str = "driver_task_reclaim_total";
        pub const DRIVER_TASK_STATE_CONFLICT_TOTAL: &str = "driver_task_state_conflict_total";

        // Derived gauges (stable names; exporters may prefix these like counters).
        pub const WRITE_SYSCALLS_PER_KIB: &str = "write_syscalls_per_kib";
        pub const WRITE_WRITEV_SHARE_RATIO: &str = "write_writev_share_ratio";
        pub const INBOUND_COPY_BYTES_PER_READ_BYTE: &str = "inbound_copy_bytes_per_read_byte";
        pub const INBOUND_COALESCES_PER_MIB: &str = "inbound_coalesces_per_mib";
    }
}
