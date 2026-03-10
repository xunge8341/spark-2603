use spark_transport::{Budget, DataPlaneConfig, DataPlaneOptions};

use crate::mgmt_profile::{MgmtHttpLimits, MgmtIsolationOptions, MgmtTransportProfileV1};

use std::net::SocketAddr;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub name: &'static str,
    /// Management plane bind address.
    pub mgmt_addr: SocketAddr,
    /// Soft shutdown timeout after entering Draining.
    pub shutdown_timeout: Duration,

    /// Max request-head bytes (request-line + headers, including CRLFs).
    pub max_head_bytes: usize,
    /// Max header count.
    pub max_headers: usize,
    /// Max request body bytes (Content-Length cap).
    pub max_body_bytes: usize,
    /// Max accepted HTTP request size (headers + body).
    pub max_request_bytes: usize,

    /// Max concurrent in-flight management requests.
    pub mgmt_max_inflight: usize,

    /// Management dataplane capacity.
    pub mgmt_max_channels: usize,
    pub mgmt_max_accept_per_tick: usize,

    /// Management dataplane poll budget.
    pub mgmt_budget: Budget,
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
        MgmtTransportProfileV1 {
            bind: self.mgmt_addr,
            shutdown_timeout: self.shutdown_timeout,
            http: MgmtHttpLimits {
                max_head_bytes: self.max_head_bytes,
                max_headers: self.max_headers,
                max_body_bytes: self.max_body_bytes,
                max_request_bytes: self.max_request_bytes,
            },
            isolation: MgmtIsolationOptions {
                max_inflight: self.mgmt_max_inflight,
                limits: spark_transport::DataPlaneLimits {
                    max_channels: self.mgmt_max_channels,
                    max_accept_per_tick: self.mgmt_max_accept_per_tick,
                },
                budget: self.mgmt_budget,
            },
            emit_evidence_log: false,
        }
    }
    #[inline]
    pub fn with_name(mut self, name: &'static str) -> Self {
        self.name = name;
        self
    }

    #[inline]
    pub fn with_mgmt_addr(mut self, mgmt_addr: SocketAddr) -> Self {
        self.mgmt_addr = mgmt_addr;
        self
    }

    #[inline]
    pub fn with_shutdown_timeout(mut self, shutdown_timeout: Duration) -> Self {
        self.shutdown_timeout = shutdown_timeout.max(Duration::from_millis(1));
        self
    }

    #[inline]
    pub fn with_max_head_bytes(mut self, max_head_bytes: usize) -> Self {
        self.max_head_bytes = max_head_bytes.max(1);
        self
    }

    #[inline]
    pub fn with_max_headers(mut self, max_headers: usize) -> Self {
        self.max_headers = max_headers.max(1);
        self
    }

    #[inline]
    pub fn with_max_body_bytes(mut self, max_body_bytes: usize) -> Self {
        self.max_body_bytes = max_body_bytes;
        self
    }

    #[inline]
    pub fn with_max_request_bytes(mut self, max_request_bytes: usize) -> Self {
        self.max_request_bytes = max_request_bytes.max(1);
        self
    }

    #[inline]
    pub fn with_mgmt_max_inflight(mut self, mgmt_max_inflight: usize) -> Self {
        self.mgmt_max_inflight = mgmt_max_inflight.max(1);
        self
    }

    #[inline]
    pub fn with_mgmt_max_channels(mut self, mgmt_max_channels: usize) -> Self {
        self.mgmt_max_channels = mgmt_max_channels.max(1);
        self
    }

    #[inline]
    pub fn with_mgmt_max_accept_per_tick(mut self, mgmt_max_accept_per_tick: usize) -> Self {
        self.mgmt_max_accept_per_tick = mgmt_max_accept_per_tick.max(1);
        self
    }

    #[inline]
    pub fn with_mgmt_budget(mut self, mgmt_budget: Budget) -> Self {
        self.mgmt_budget = mgmt_budget;
        self
    }

    /// Effective max request bytes used by transport framing.
    ///
    /// This ensures framing bounds cover the configured head/body limits.
    #[inline]
    pub fn effective_max_request_bytes(&self) -> usize {
        let min_total = self.max_head_bytes.saturating_add(self.max_body_bytes).max(1);
        self.max_request_bytes.max(min_total)
    }

    /// Build the canonical transport-backed management profile.
    ///
    /// This keeps dogfooding defaults in one place so the hosting layer and the Ember transport
    /// server share the same capacity, framing, and shutdown semantics.
    #[inline]
    pub fn management_transport_options(&self) -> DataPlaneOptions {
        self.mgmt_profile_v1().transport_options()
    }

    /// Build the canonical transport-backed management profile as a runtime config.
    #[inline]
    pub fn management_transport_config(&self) -> DataPlaneConfig {
        self.management_transport_options().build()
    }

    /// Build the canonical transport-backed management profile with throughput-oriented tuning.
    #[inline]
    pub fn management_transport_perf_options(&self) -> DataPlaneOptions {
        self.mgmt_profile_v1().transport_options().with_perf_defaults()
    }

    /// Build the canonical transport-backed management profile with throughput-oriented tuning as a
    /// runtime config.
    #[inline]
    pub fn management_transport_perf_config(&self) -> DataPlaneConfig {
        self.mgmt_profile_v1().transport_perf_config()
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            name: "spark-ember",
            mgmt_addr: SocketAddr::from(([127, 0, 0, 1], 8080)),
            shutdown_timeout: Duration::from_secs(10),
            max_head_bytes: 16 * 1024,
            max_headers: 64,
            max_body_bytes: 48 * 1024,
            max_request_bytes: 64 * 1024,
            mgmt_max_inflight: 128,
            mgmt_max_channels: 1024,
            mgmt_max_accept_per_tick: 64,
            mgmt_budget: Budget {
                max_events: 128,
                max_nanos: 1_000_000,
            },
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
        assert_eq!(dp.bind, cfg.mgmt_addr);
        assert_eq!(dp.max_channels, 256);
        assert_eq!(dp.max_accept_per_tick, 4);
        assert_eq!(dp.drain_timeout, cfg.shutdown_timeout);
        assert!(!dp.emit_evidence_log);
        assert_eq!(dp.max_frame_hint(), cfg.effective_max_request_bytes());
        match dp.framing {
            spark_transport::async_bridge::FrameDecoderProfile::Http1 { max_request_bytes, max_head_bytes, max_headers } => {
                assert_eq!(max_request_bytes, cfg.effective_max_request_bytes());
                assert_eq!(max_head_bytes, cfg.max_head_bytes.min(cfg.effective_max_request_bytes()));
                assert_eq!(max_headers, cfg.max_headers.max(1));
            }
            other => panic!("expected Http1 framing, got {other:?}"),
        }
        assert_eq!(dp.budget.max_events, cfg.mgmt_budget.max_events);
        assert_eq!(dp.budget.max_nanos, cfg.mgmt_budget.max_nanos);
        assert!(dp.validate().is_ok());
    }

    #[test]
    fn management_transport_perf_profile_applies_perf_overlay() {
        let cfg = ServerConfig::default()
            .with_mgmt_addr("127.0.0.1:19090".parse().expect("static addr"))
            .with_shutdown_timeout(Duration::from_secs(2))
            .with_max_request_bytes(16 * 1024);

        let dp = cfg.management_transport_perf_config();
        assert_eq!(dp.bind, cfg.mgmt_addr);
        assert_eq!(dp.max_frame_hint(), cfg.effective_max_request_bytes());
        assert_eq!(dp.drain_timeout, cfg.shutdown_timeout);
        assert_eq!(dp.flush_policy.max_syscalls, 64);
        assert_eq!(dp.watermark.low_mul, 8);
        assert_eq!(dp.watermark.high_mul, 16);
        assert_eq!(dp.budget.max_events, 512);
        assert!(dp.validate().is_ok());
    }
}
