use spark_transport::{Budget, DataPlaneConfig, DataPlaneOptions};

use crate::mgmt_profile::MgmtTransportProfileV1;

use std::net::SocketAddr;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub name: &'static str,
    pub mgmt: MgmtTransportProfileV1,
}

/// Stable, runtime-auditable host effective config snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerEffectiveConfig {
    pub mgmt_profile: crate::mgmt_profile::MgmtProfileEffectiveConfig,
    pub management_transport: spark_transport::DataPlaneEffectiveConfig,
    pub management_transport_perf_overlay: spark_transport::DataPlaneEffectiveConfig,
}

impl ServerConfig {
    /// Build a canonical mgmt-plane profile (v1).
    ///
    /// This profile is used by:
    /// - transport-backed mgmt server (dogfooding);
    /// - host builder helpers;
    /// - cross-platform contract and smoke tests.
    #[inline]
    pub fn mgmt_profile_v1(&self) -> MgmtTransportProfileV1 {
        self.mgmt.clone()
    }
    #[inline]
    pub fn with_name(mut self, name: &'static str) -> Self {
        self.name = name;
        self
    }

    #[inline]
    pub fn with_mgmt_addr(mut self, mgmt_addr: SocketAddr) -> Self {
        self.mgmt.bind = mgmt_addr;
        self
    }

    #[inline]
    pub fn with_shutdown_timeout(mut self, shutdown_timeout: Duration) -> Self {
        self.mgmt.shutdown_timeout = shutdown_timeout.max(Duration::from_millis(1));
        self
    }

    #[inline]
    pub fn with_max_head_bytes(mut self, max_head_bytes: usize) -> Self {
        self.mgmt.http.max_head_bytes = max_head_bytes.max(1);
        self
    }

    #[inline]
    pub fn with_max_headers(mut self, max_headers: usize) -> Self {
        self.mgmt.http.max_headers = max_headers.max(1);
        self
    }

    #[inline]
    pub fn with_max_body_bytes(mut self, max_body_bytes: usize) -> Self {
        self.mgmt.http.max_body_bytes = max_body_bytes;
        self
    }

    #[inline]
    pub fn with_max_request_bytes(mut self, max_request_bytes: usize) -> Self {
        self.mgmt.http.max_request_bytes = max_request_bytes.max(1);
        self
    }

    #[inline]
    pub fn with_mgmt_max_inflight(mut self, mgmt_max_inflight: usize) -> Self {
        self.mgmt.isolation.max_inflight = mgmt_max_inflight.max(1);
        self
    }

    #[inline]
    pub fn with_mgmt_max_channels(mut self, mgmt_max_channels: usize) -> Self {
        self.mgmt.isolation.limits.max_channels = mgmt_max_channels.max(1);
        self
    }

    #[inline]
    pub fn with_mgmt_max_accept_per_tick(mut self, mgmt_max_accept_per_tick: usize) -> Self {
        self.mgmt.isolation.limits.max_accept_per_tick = mgmt_max_accept_per_tick.max(1);
        self
    }

    #[inline]
    pub fn with_mgmt_budget(mut self, mgmt_budget: Budget) -> Self {
        self.mgmt.isolation.budget = mgmt_budget;
        self
    }

    #[inline]
    pub fn with_connection_idle_timeout(mut self, timeout: Duration) -> Self {
        self.mgmt.connection_timeouts.idle_timeout = timeout.max(Duration::from_millis(1));
        self
    }

    #[inline]
    pub fn with_connection_read_timeout(mut self, timeout: Duration) -> Self {
        self.mgmt.connection_timeouts.read_timeout = timeout.max(Duration::from_millis(1));
        self
    }

    #[inline]
    pub fn with_connection_write_timeout(mut self, timeout: Duration) -> Self {
        self.mgmt.connection_timeouts.write_timeout = timeout.max(Duration::from_millis(1));
        self
    }

    #[inline]
    pub fn with_request_headers_timeout(mut self, timeout: Duration) -> Self {
        self.mgmt.connection_timeouts.request_headers_timeout =
            timeout.max(Duration::from_millis(1));
        self
    }

    #[inline]
    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.mgmt.request_timeouts.default_timeout = timeout.max(Duration::from_millis(1));
        self
    }

    #[inline]
    pub fn with_max_concurrent_requests(mut self, limit: usize) -> Self {
        self.mgmt.overload.max_concurrent_requests = limit.max(1);
        self
    }

    #[inline]
    pub fn with_max_inflight_per_connection(mut self, limit: usize) -> Self {
        self.mgmt.overload.max_inflight_per_connection = limit.max(1);
        self
    }

    #[inline]
    pub fn with_request_queue_limit(mut self, limit: usize) -> Self {
        self.mgmt.overload.queue_limit = limit;
        self
    }

    #[inline]
    pub fn with_reject_policy(mut self, policy: crate::mgmt_profile::MgmtRejectPolicy) -> Self {
        self.mgmt.overload.reject_policy = policy;
        self
    }

    /// Effective max request bytes used by transport framing.
    ///
    /// This ensures framing bounds cover the configured head/body limits.
    #[inline]
    pub fn effective_max_request_bytes(&self) -> usize {
        self.mgmt.http.effective_max_request_bytes()
    }

    /// Build the canonical transport-backed management profile.
    ///
    /// This keeps dogfooding defaults in one place so the hosting layer and the Ember transport
    /// server share the same capacity, framing, and shutdown semantics.
    #[inline]
    pub fn management_transport_options(&self) -> DataPlaneOptions {
        self.mgmt.transport_options()
    }

    /// Build the canonical transport-backed management profile as a runtime config.
    #[inline]
    pub fn management_transport_config(&self) -> DataPlaneConfig {
        self.mgmt.transport_config()
    }

    /// Build the canonical transport-backed management profile with throughput-oriented tuning.
    #[inline]
    pub fn management_transport_perf_options(&self) -> DataPlaneOptions {
        self.mgmt.transport_options().with_perf_defaults()
    }

    /// Build the canonical transport-backed management profile with throughput-oriented tuning as a
    /// runtime config.
    #[inline]
    pub fn management_transport_perf_config(&self) -> DataPlaneConfig {
        self.mgmt.transport_perf_config()
    }

    /// Build a stable, runtime-auditable host effective config snapshot.
    #[inline]
    pub fn describe_effective_config(&self) -> ServerEffectiveConfig {
        ServerEffectiveConfig {
            mgmt_profile: self.mgmt.describe_effective_config(),
            management_transport: self.management_transport_config().describe_effective(),
            management_transport_perf_overlay: self
                .management_transport_perf_config()
                .describe_effective(),
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            name: "spark-ember",
            mgmt: MgmtTransportProfileV1::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ServerConfig;
    use spark_transport::Budget;
    use std::time::Duration;

    #[test]
    fn management_transport_profile_tracks_server_settings() {
        let cfg = ServerConfig::default()
            .with_mgmt_addr("127.0.0.1:18080".parse().expect("static addr"))
            .with_shutdown_timeout(Duration::from_secs(3))
            .with_max_head_bytes(4096)
            .with_max_headers(8)
            .with_max_body_bytes(4096)
            .with_max_request_bytes(8192)
            .with_mgmt_max_channels(256)
            .with_mgmt_max_accept_per_tick(4)
            .with_mgmt_budget(Budget {
                max_events: 64,
                max_nanos: 500_000,
            });

        let dp = cfg.management_transport_config();
        assert_eq!(dp.bind, cfg.mgmt.bind);
        assert_eq!(dp.max_channels, 256);
        assert_eq!(dp.max_accept_per_tick, 4);
        assert_eq!(dp.drain_timeout, cfg.mgmt.shutdown_timeout);
        assert!(!dp.emit_evidence_log);
        assert_eq!(dp.max_frame_hint(), cfg.effective_max_request_bytes());
        match dp.framing {
            spark_transport::async_bridge::FrameDecoderProfile::Http1 {
                max_request_bytes,
                max_head_bytes,
                max_headers,
            } => {
                assert_eq!(max_request_bytes, cfg.effective_max_request_bytes());
                assert_eq!(
                    max_head_bytes,
                    cfg.mgmt
                        .http
                        .max_head_bytes
                        .min(cfg.effective_max_request_bytes())
                );
                assert_eq!(max_headers, cfg.mgmt.http.max_headers.max(1));
            }
            other => panic!("expected Http1 framing, got {other:?}"),
        }
        assert_eq!(dp.budget.max_events, cfg.mgmt.isolation.budget.max_events);
        assert_eq!(dp.budget.max_nanos, cfg.mgmt.isolation.budget.max_nanos);
        assert!(dp.validate().is_ok());
    }

    #[test]
    fn management_transport_perf_profile_applies_perf_overlay() {
        let cfg = ServerConfig::default()
            .with_mgmt_addr("127.0.0.1:19090".parse().expect("static addr"))
            .with_shutdown_timeout(Duration::from_secs(2))
            .with_max_request_bytes(16 * 1024);

        let dp = cfg.management_transport_perf_config();
        assert_eq!(dp.bind, cfg.mgmt.bind);
        assert_eq!(dp.max_frame_hint(), cfg.effective_max_request_bytes());
        assert_eq!(dp.drain_timeout, cfg.mgmt.shutdown_timeout);
        assert_eq!(dp.flush_policy.max_syscalls, 64);
        assert_eq!(dp.watermark.low_mul, 8);
        assert_eq!(dp.watermark.high_mul, 16);
        assert_eq!(dp.budget.max_events, 512);
        assert!(dp.validate().is_ok());
    }

    #[test]
    fn server_describe_effective_config_explains_default_and_perf() {
        let cfg = ServerConfig::default()
            .with_mgmt_addr("127.0.0.1:29090".parse().expect("static addr"))
            .with_max_request_bytes(8 * 1024)
            .with_mgmt_max_channels(128)
            .with_mgmt_max_accept_per_tick(9);

        let effective = cfg.describe_effective_config();
        assert_eq!(effective.mgmt_profile.bind, cfg.mgmt.bind);
        assert_eq!(effective.management_transport.bind, cfg.mgmt.bind);
        assert_eq!(effective.management_transport.max_channels, 128);
        assert_eq!(effective.management_transport.backlog, 9);
        assert_eq!(
            effective.management_transport.max_frame_hint,
            effective.mgmt_profile.effective_max_request_bytes
        );
        assert_eq!(
            effective.management_transport_perf_overlay.bind,
            cfg.mgmt.bind
        );
        assert_eq!(
            effective.management_transport_perf_overlay.max_channels,
            128
        );
        assert_eq!(effective.management_transport_perf_overlay.backlog, 9);
        assert_eq!(
            effective
                .management_transport_perf_overlay
                .flush_policy
                .max_syscalls,
            64
        );
        assert_eq!(
            effective
                .management_transport_perf_overlay
                .budget
                .max_events,
            512
        );
    }
}
