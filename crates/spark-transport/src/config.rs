use crate::async_bridge::FrameDecoderProfile;
use crate::policy::{FlushPolicy, WatermarkPolicy};
use crate::Budget;
use std::net::SocketAddr;
use std::time::Duration;

/// Dataplane sizing/capacity knobs.
///
/// This is the smallest “day-1” subset we want most users to care about:
/// - how many channels we can host;
/// - how many accepts we attempt per tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataPlaneLimits {
    pub max_channels: usize,
    pub max_accept_per_tick: usize,
}

impl Default for DataPlaneLimits {
    fn default() -> Self {
        Self {
            max_channels: 4096,
            max_accept_per_tick: 64,
        }
    }
}

impl DataPlaneLimits {
    #[inline]
    pub fn normalized(mut self) -> Self {
        self.max_channels = self.max_channels.max(1);
        self.max_accept_per_tick = self.max_accept_per_tick.max(1);
        self
    }
}

/// Dataplane diagnostics knobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DataPlaneDiagnostics {
    /// Whether to emit best-effort evidence logs.
    pub emit_evidence_log: bool,
}

/// Validation errors for product-level dataplane configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataPlaneConfigError {
    ZeroMaxChannels,
    ZeroMaxAcceptPerTick,
    ZeroDrainTimeout,
    InvalidWatermarkPolicy,
    InvalidFlushPolicy,
}

impl core::fmt::Display for DataPlaneConfigError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let msg = match self {
            Self::ZeroMaxChannels => "max_channels must be greater than zero",
            Self::ZeroMaxAcceptPerTick => "max_accept_per_tick must be greater than zero",
            Self::ZeroDrainTimeout => "drain_timeout must be greater than zero",
            Self::InvalidWatermarkPolicy => "watermark policy must satisfy high_mul > low_mul >= 1",
            Self::InvalidFlushPolicy => {
                "flush policy must satisfy max_bytes_mul >= 1, max_bytes_cap >= 1, max_syscalls >= 1, max_iov >= 1"
            }
        };
        f.write_str(msg)
    }
}

impl std::error::Error for DataPlaneConfigError {}

/// Product-level configuration surface.
///
/// This is an Options-style façade over [`DataPlaneConfig`].
/// It keeps the mature core config available, while giving callers a clearer “few knobs first”
/// surface similar to ASP.NET Core’s hosting options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPlaneOptions {
    pub bind: SocketAddr,
    pub limits: DataPlaneLimits,
    /// Backpressure watermarks policy (outbound buffer).
    pub watermark: WatermarkPolicy,
    /// Flush fairness policy (per-tick write budget).
    pub flush_policy: FlushPolicy,
    /// Stream/datagram framing profile.
    pub framing: FrameDecoderProfile,
    pub budget: Budget,
    /// Graceful draining timeout used when the dataplane enters shutdown.
    pub drain_timeout: Duration,
    /// Hard cap for per-channel pending outbound bytes (`usize::MAX` keeps compatibility).
    pub max_pending_write_bytes: usize,
    pub diagnostics: DataPlaneDiagnostics,
}

impl DataPlaneOptions {
    /// Build a TCP-friendly baseline profile.
    #[inline]
    pub fn tcp(bind: SocketAddr) -> Self {
        Self {
            bind,
            ..Self::default()
        }
    }

    /// Build a management-plane HTTP/1 profile.
    ///
    /// This intentionally aligns the framing and bind address so callers do not have to hand-edit
    /// multiple fields for the most common dogfooding scenario.
    #[inline]
    pub fn management_http(bind: SocketAddr, max_request_bytes: usize) -> Self {
        Self::tcp(bind).with_framing(FrameDecoderProfile::http1(max_request_bytes))
    }

    /// Build a management-plane HTTP/1 profile with explicit head limits.
    #[inline]
    pub fn management_http_with_limits(
        bind: SocketAddr,
        max_request_bytes: usize,
        max_head_bytes: usize,
        max_headers: usize,
    ) -> Self {
        Self::tcp(bind).with_framing(FrameDecoderProfile::http1_with_limits(
            max_request_bytes,
            max_head_bytes,
            max_headers,
        ))
    }

    /// Build a throughput-oriented TCP profile.
    #[inline]
    pub fn perf_tcp(bind: SocketAddr) -> Self {
        Self::tcp(bind).with_perf_defaults()
    }

    #[inline]
    pub fn normalized(mut self) -> Self {
        self.limits = self.limits.normalized();
        self.drain_timeout = self.drain_timeout.max(Duration::from_millis(1));
        self.max_pending_write_bytes = self.max_pending_write_bytes.max(1);
        self
    }

    #[inline]
    pub fn validate(&self) -> core::result::Result<(), DataPlaneConfigError> {
        DataPlaneConfig::from(self.clone().normalized()).validate()
    }

    #[inline]
    pub fn build(self) -> DataPlaneConfig {
        self.normalized().into()
    }

    /// Apply a conservative throughput-oriented tuning overlay.
    ///
    /// This keeps bind/framing/limits intact while increasing flush and reactor budgets enough for
    /// local perf bring-up and coarse regression checks.
    #[inline]
    pub fn with_perf_defaults(mut self) -> Self {
        self.watermark = WatermarkPolicy {
            min_frame: 4 * 1024,
            high_mul: 16,
            low_mul: 8,
        };
        self.flush_policy = FlushPolicy {
            min_frame: 64 * 1024,
            max_bytes_mul: 64,
            max_bytes_cap: 64 * 1024 * 1024,
            max_syscalls: 64,
            max_iov: 16,
        };
        self.budget = Budget {
            max_events: 512,
            max_nanos: 4_000_000,
        };
        self.diagnostics.emit_evidence_log = false;
        self
    }

    #[inline]
    pub fn with_bind(mut self, bind: SocketAddr) -> Self {
        self.bind = bind;
        self
    }

    #[inline]
    pub fn with_limits(mut self, limits: DataPlaneLimits) -> Self {
        self.limits = limits;
        self
    }

    #[inline]
    pub fn with_max_channels(mut self, max_channels: usize) -> Self {
        self.limits.max_channels = max_channels;
        self
    }

    #[inline]
    pub fn with_max_accept_per_tick(mut self, max_accept_per_tick: usize) -> Self {
        self.limits.max_accept_per_tick = max_accept_per_tick;
        self
    }

    #[inline]
    pub fn with_framing(mut self, framing: FrameDecoderProfile) -> Self {
        self.framing = framing;
        self
    }

    #[inline]
    pub fn with_watermark(mut self, watermark: WatermarkPolicy) -> Self {
        self.watermark = watermark;
        self
    }

    #[inline]
    pub fn with_flush_policy(mut self, flush_policy: FlushPolicy) -> Self {
        self.flush_policy = flush_policy;
        self
    }

    #[inline]
    pub fn with_budget(mut self, budget: Budget) -> Self {
        self.budget = budget;
        self
    }

    #[inline]
    pub fn with_drain_timeout(mut self, drain_timeout: Duration) -> Self {
        self.drain_timeout = drain_timeout;
        self
    }

    #[inline]
    pub fn with_max_pending_write_bytes(mut self, max_pending_write_bytes: usize) -> Self {
        self.max_pending_write_bytes = max_pending_write_bytes;
        self
    }

    #[inline]
    pub fn with_diagnostics(mut self, diagnostics: DataPlaneDiagnostics) -> Self {
        self.diagnostics = diagnostics;
        self
    }

    #[inline]
    pub fn with_evidence_log(mut self, emit_evidence_log: bool) -> Self {
        self.diagnostics.emit_evidence_log = emit_evidence_log;
        self
    }
}

impl Default for DataPlaneOptions {
    fn default() -> Self {
        Self::from(DataPlaneConfig::default())
    }
}

/// Dataplane configuration (runtime-neutral).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPlaneConfig {
    pub bind: SocketAddr,
    pub max_channels: usize,
    /// Backpressure watermarks policy (outbound buffer).
    pub watermark: WatermarkPolicy,
    /// Flush fairness policy (per-tick write budget).
    pub flush_policy: FlushPolicy,
    /// Stream/datagram framing profile.
    ///
    /// For TCP, this drives the built-in stream decoder/encoder pair.
    /// For UDP, this value is used as a datagram size hint (buffer sizing / max payload).
    pub framing: FrameDecoderProfile,
    pub budget: Budget,
    pub max_accept_per_tick: usize,
    /// Graceful draining timeout used when the dataplane enters shutdown.
    pub drain_timeout: Duration,
    /// Hard cap for per-channel pending outbound bytes (`usize::MAX` keeps compatibility).
    pub max_pending_write_bytes: usize,
    /// Whether to emit best-effort evidence logs.
    pub emit_evidence_log: bool,
}

impl DataPlaneConfig {
    /// Build a TCP-friendly baseline profile.
    #[inline]
    pub fn tcp(bind: SocketAddr) -> Self {
        Self {
            bind,
            ..Self::default()
        }
    }

    /// Build a management-plane HTTP/1 profile.
    #[inline]
    pub fn management_http(bind: SocketAddr, max_request_bytes: usize) -> Self {
        Self::tcp(bind).with_framing(FrameDecoderProfile::http1(max_request_bytes))
    }

    /// Build a management-plane HTTP/1 profile with explicit head limits.
    #[inline]
    pub fn management_http_with_limits(
        bind: SocketAddr,
        max_request_bytes: usize,
        max_head_bytes: usize,
        max_headers: usize,
    ) -> Self {
        Self::tcp(bind).with_framing(FrameDecoderProfile::http1_with_limits(
            max_request_bytes,
            max_head_bytes,
            max_headers,
        ))
    }

    /// Build a throughput-oriented TCP profile.
    #[inline]
    pub fn perf_tcp(bind: SocketAddr) -> Self {
        Self::tcp(bind).with_perf_defaults()
    }

    /// Return a normalized copy.
    ///
    /// Public fields keep the type easy to use in tests/bring-up, but the runtime should never run
    /// with zero capacities or a zero drain timeout. This helper clamps the known foot-guns.
    #[inline]
    pub fn normalized(mut self) -> Self {
        self.max_channels = self.max_channels.max(1);
        self.max_accept_per_tick = self.max_accept_per_tick.max(1);
        self.drain_timeout = self.drain_timeout.max(Duration::from_millis(1));
        self.max_pending_write_bytes = self.max_pending_write_bytes.max(1);
        self
    }

    /// Validate structural invariants for a dataplane configuration.
    #[inline]
    pub fn validate(&self) -> core::result::Result<(), DataPlaneConfigError> {
        if self.max_channels == 0 {
            return Err(DataPlaneConfigError::ZeroMaxChannels);
        }
        if self.max_accept_per_tick == 0 {
            return Err(DataPlaneConfigError::ZeroMaxAcceptPerTick);
        }
        if self.drain_timeout.is_zero() {
            return Err(DataPlaneConfigError::ZeroDrainTimeout);
        }
        if self.watermark.low_mul == 0 || self.watermark.high_mul <= self.watermark.low_mul {
            return Err(DataPlaneConfigError::InvalidWatermarkPolicy);
        }
        if self.flush_policy.max_bytes_mul == 0
            || self.flush_policy.max_bytes_cap == 0
            || self.flush_policy.max_syscalls == 0
        {
            return Err(DataPlaneConfigError::InvalidFlushPolicy);
        }
        Ok(())
    }

    /// Maximum frame/datagram size hint derived from the framing profile.
    #[inline]
    pub fn max_frame_hint(&self) -> usize {
        self.framing.max_frame_hint().max(1)
    }

    /// Apply a conservative throughput-oriented tuning overlay.
    ///
    /// This keeps bind/framing/limits intact while increasing flush and reactor budgets enough for
    /// local perf bring-up and coarse regression checks.
    #[inline]
    pub fn with_perf_defaults(mut self) -> Self {
        self.watermark = WatermarkPolicy {
            min_frame: 4 * 1024,
            high_mul: 16,
            low_mul: 8,
        };
        self.flush_policy = FlushPolicy {
            min_frame: 64 * 1024,
            max_bytes_mul: 64,
            max_bytes_cap: 64 * 1024 * 1024,
            max_syscalls: 64,
            max_iov: 16,
        };
        self.budget = Budget {
            max_events: 512,
            max_nanos: 4_000_000,
        };
        self.emit_evidence_log = false;
        self
    }

    /// Set the bind address (Options-style).
    #[inline]
    pub fn with_bind(mut self, bind: SocketAddr) -> Self {
        self.bind = bind;
        self
    }

    /// Set the framing profile (Options-style).
    #[inline]
    pub fn with_framing(mut self, framing: FrameDecoderProfile) -> Self {
        self.framing = framing;
        self
    }

    /// Set the watermark policy (Options-style).
    #[inline]
    pub fn with_watermark(mut self, watermark: WatermarkPolicy) -> Self {
        self.watermark = watermark;
        self
    }

    /// Set the flush policy (Options-style).
    #[inline]
    pub fn with_flush_policy(mut self, flush_policy: FlushPolicy) -> Self {
        self.flush_policy = flush_policy;
        self
    }

    /// Set the channel capacity (Options-style).
    #[inline]
    pub fn with_max_channels(mut self, max_channels: usize) -> Self {
        self.max_channels = max_channels;
        self
    }

    /// Set the poll budget (Options-style).
    #[inline]
    pub fn with_budget(mut self, budget: Budget) -> Self {
        self.budget = budget;
        self
    }

    /// Set the accept cap per tick (Options-style).
    #[inline]
    pub fn with_max_accept_per_tick(mut self, max_accept_per_tick: usize) -> Self {
        self.max_accept_per_tick = max_accept_per_tick;
        self
    }

    /// Set the graceful drain timeout (Options-style).
    #[inline]
    pub fn with_drain_timeout(mut self, drain_timeout: Duration) -> Self {
        self.drain_timeout = drain_timeout;
        self
    }

    /// Set pending outbound hard cap (Options-style).
    #[inline]
    pub fn with_max_pending_write_bytes(mut self, max_pending_write_bytes: usize) -> Self {
        self.max_pending_write_bytes = max_pending_write_bytes;
        self
    }

    /// Set evidence logging (Options-style).
    #[inline]
    pub fn with_evidence_log(mut self, emit_evidence_log: bool) -> Self {
        self.emit_evidence_log = emit_evidence_log;
        self
    }
}

impl Default for DataPlaneConfig {
    fn default() -> Self {
        Self {
            bind: SocketAddr::from(([0, 0, 0, 0], 5060)),
            max_channels: 4096,
            watermark: WatermarkPolicy::default(),
            flush_policy: FlushPolicy::default(),
            framing: FrameDecoderProfile::line(64 * 1024),
            budget: Budget {
                max_events: 256,
                max_nanos: 2_000_000,
            },
            max_accept_per_tick: 64,
            drain_timeout: Duration::from_secs(5),
            max_pending_write_bytes: usize::MAX,
            emit_evidence_log: false,
        }
    }
}

impl From<DataPlaneOptions> for DataPlaneConfig {
    #[inline]
    fn from(value: DataPlaneOptions) -> Self {
        let limits = value.limits.normalized();
        Self {
            bind: value.bind,
            max_channels: limits.max_channels,
            watermark: value.watermark,
            flush_policy: value.flush_policy,
            framing: value.framing,
            budget: value.budget,
            max_accept_per_tick: limits.max_accept_per_tick,
            drain_timeout: value.drain_timeout.max(Duration::from_millis(1)),
            max_pending_write_bytes: value.max_pending_write_bytes.max(1),
            emit_evidence_log: value.diagnostics.emit_evidence_log,
        }
    }
}

impl From<DataPlaneConfig> for DataPlaneOptions {
    #[inline]
    fn from(value: DataPlaneConfig) -> Self {
        Self {
            bind: value.bind,
            limits: DataPlaneLimits {
                max_channels: value.max_channels,
                max_accept_per_tick: value.max_accept_per_tick,
            },
            watermark: value.watermark,
            flush_policy: value.flush_policy,
            framing: value.framing,
            budget: value.budget,
            drain_timeout: value.drain_timeout,
            max_pending_write_bytes: value.max_pending_write_bytes,
            diagnostics: DataPlaneDiagnostics {
                emit_evidence_log: value.emit_evidence_log,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DataPlaneConfig, DataPlaneConfigError, DataPlaneOptions};
    use crate::async_bridge::FrameDecoderProfile;
    use crate::policy::{FlushPolicy, WatermarkPolicy};
    use std::time::Duration;

    #[test]
    fn dataplane_options_build_normalizes_numeric_footguns() {
        let cfg = DataPlaneOptions::default()
            .with_max_channels(0)
            .with_max_accept_per_tick(0)
            .with_drain_timeout(Duration::ZERO)
            .with_max_pending_write_bytes(0)
            .build();

        assert_eq!(cfg.max_channels, 1);
        assert_eq!(cfg.max_accept_per_tick, 1);
        assert_eq!(cfg.drain_timeout, Duration::from_millis(1));
        assert_eq!(cfg.max_pending_write_bytes, 1);
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn dataplane_config_validate_rejects_invalid_watermark_policy() {
        let cfg = DataPlaneConfig::default().with_watermark(WatermarkPolicy {
            min_frame: 1024,
            high_mul: 4,
            low_mul: 4,
        });

        assert_eq!(
            cfg.validate(),
            Err(DataPlaneConfigError::InvalidWatermarkPolicy)
        );
    }

    #[test]
    fn dataplane_config_validate_rejects_invalid_flush_policy() {
        let cfg = DataPlaneConfig::default().with_flush_policy(FlushPolicy {
            min_frame: 64 * 1024,
            max_bytes_mul: 0,
            max_bytes_cap: 16 * 1024 * 1024,
            max_syscalls: 32,
            max_iov: 8,
        });

        assert_eq!(
            cfg.validate(),
            Err(DataPlaneConfigError::InvalidFlushPolicy)
        );
    }

    #[test]
    fn dataplane_perf_overlay_preserves_shape_and_tunes_budgets() {
        let bind = "127.0.0.1:25060".parse().expect("static addr");
        let cfg = DataPlaneOptions::management_http(bind, 8192)
            .with_max_channels(333)
            .with_max_accept_per_tick(11)
            .with_drain_timeout(Duration::from_secs(7))
            .with_evidence_log(true)
            .with_perf_defaults()
            .build();

        assert_eq!(cfg.bind, bind);
        assert_eq!(cfg.max_channels, 333);
        assert_eq!(cfg.max_accept_per_tick, 11);
        assert_eq!(cfg.drain_timeout, Duration::from_secs(7));
        assert_eq!(cfg.framing, FrameDecoderProfile::http1(8192));
        assert!(!cfg.emit_evidence_log);
        assert_eq!(cfg.watermark.low_mul, 8);
        assert_eq!(cfg.watermark.high_mul, 16);
        assert_eq!(cfg.flush_policy.max_syscalls, 64);
        assert_eq!(cfg.flush_policy.max_iov, 16);
        assert_eq!(cfg.budget.max_events, 512);
        assert_eq!(cfg.budget.max_nanos, 4_000_000);
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn dataplane_perf_tcp_helper_matches_overlay() {
        let bind = "127.0.0.1:26060".parse().expect("static addr");
        assert_eq!(
            DataPlaneConfig::perf_tcp(bind),
            DataPlaneConfig::tcp(bind).with_perf_defaults()
        );
        assert_eq!(
            DataPlaneOptions::perf_tcp(bind).build(),
            DataPlaneConfig::perf_tcp(bind)
        );
    }
}
