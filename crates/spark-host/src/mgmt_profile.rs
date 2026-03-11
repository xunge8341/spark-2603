//! Management-plane profile (v1).
//!
//! Goals:
//! - Provide a single, stable place to define mgmt-plane HTTP limits, isolation, and transport knobs.
//! - Keep the profile runtime-neutral and Options-style, aligned with ASP.NET Core hosting ergonomics.
//! - Make dogfooding reproducible: transport-backed mgmt must derive its config from this profile.

use spark_transport::{Budget, DataPlaneConfig, DataPlaneLimits, DataPlaneOptions};

use std::net::SocketAddr;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MgmtRejectPolicy {
    ServiceUnavailable,
    TooManyRequests,
    CloseConnection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MgmtConnectionTimeouts {
    pub idle_timeout: Duration,
    pub read_timeout: Duration,
    pub write_timeout: Duration,
    pub request_headers_timeout: Duration,
}

impl Default for MgmtConnectionTimeouts {
    fn default() -> Self {
        Self {
            idle_timeout: Duration::from_secs(30),
            read_timeout: Duration::from_secs(15),
            write_timeout: Duration::from_secs(15),
            request_headers_timeout: Duration::from_secs(5),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MgmtRequestTimeouts {
    pub default_timeout: Duration,
}

impl Default for MgmtRequestTimeouts {
    fn default() -> Self {
        Self {
            default_timeout: Duration::from_secs(10),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MgmtOverloadOptions {
    pub max_concurrent_requests: usize,
    pub max_inflight_per_connection: usize,
    pub queue_limit: usize,
    pub reject_policy: MgmtRejectPolicy,
}

impl Default for MgmtOverloadOptions {
    fn default() -> Self {
        Self {
            max_concurrent_requests: 128,
            max_inflight_per_connection: 1,
            queue_limit: 32,
            reject_policy: MgmtRejectPolicy::ServiceUnavailable,
        }
    }
}

impl MgmtOverloadOptions {
    #[inline]
    pub fn normalized(mut self) -> Self {
        self.max_concurrent_requests = self.max_concurrent_requests.max(1);
        self.max_inflight_per_connection = self.max_inflight_per_connection.max(1);
        self
    }
}

/// HTTP request limits for the management plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MgmtHttpLimits {
    /// Max request-head bytes (request-line + headers, including CRLFs).
    pub max_head_bytes: usize,
    /// Max header count.
    pub max_headers: usize,
    /// Max request body bytes (Content-Length cap).
    pub max_body_bytes: usize,
    /// Max accepted HTTP request size (headers + body).
    pub max_request_bytes: usize,
}

impl MgmtHttpLimits {
    /// Effective max request bytes used by transport framing.
    ///
    /// This ensures framing bounds cover the configured head/body limits.
    #[inline]
    pub fn effective_max_request_bytes(&self) -> usize {
        let min_total = self
            .max_head_bytes
            .saturating_add(self.max_body_bytes)
            .max(1);
        self.max_request_bytes.max(min_total)
    }

    #[inline]
    pub fn normalized(mut self) -> Self {
        self.max_head_bytes = self.max_head_bytes.max(1);
        self.max_headers = self.max_headers.max(1);
        self.max_request_bytes = self.max_request_bytes.max(1);
        self
    }
}

impl Default for MgmtHttpLimits {
    fn default() -> Self {
        Self {
            max_head_bytes: 16 * 1024,
            max_headers: 64,
            max_body_bytes: 48 * 1024,
            max_request_bytes: 64 * 1024,
        }
    }
}

/// Isolation knobs for the management plane.
///
/// The mgmt plane is not a throughput dataplane; we want clean, bounded resource usage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MgmtIsolationOptions {
    /// Max concurrent in-flight mgmt requests.
    pub max_inflight: usize,
    /// Transport capacity limits.
    pub limits: DataPlaneLimits,
    /// Transport poll budget.
    pub budget: Budget,
}

impl MgmtIsolationOptions {
    #[inline]
    pub fn normalized(mut self) -> Self {
        self.max_inflight = self.max_inflight.max(1);
        self.limits = self.limits.normalized();
        self
    }
}

impl Default for MgmtIsolationOptions {
    fn default() -> Self {
        Self {
            max_inflight: 128,
            limits: DataPlaneLimits {
                max_channels: 1024,
                max_accept_per_tick: 64,
            },
            budget: Budget {
                max_events: 128,
                max_nanos: 1_000_000,
            },
        }
    }
}

/// Canonical management-plane profile (v1).
///
/// This profile is the single source of truth for:
/// - HTTP request limits (framing + decoding);
/// - isolation (in-flight cap + transport capacity);
/// - transport behavior (drain timeout, evidence logging).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MgmtTransportProfileV1 {
    pub bind: SocketAddr,
    pub shutdown_timeout: Duration,
    pub http: MgmtHttpLimits,
    pub isolation: MgmtIsolationOptions,
    pub connection_timeouts: MgmtConnectionTimeouts,
    pub request_timeouts: MgmtRequestTimeouts,
    pub overload: MgmtOverloadOptions,
    /// Whether the mgmt transport emits best-effort evidence logs.
    pub emit_evidence_log: bool,
}

/// Stable, runtime-auditable mgmt profile effective snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MgmtProfileEffectiveConfig {
    pub bind: SocketAddr,
    pub shutdown_timeout: Duration,
    pub http: MgmtHttpLimits,
    pub effective_max_request_bytes: usize,
    pub isolation: MgmtIsolationOptions,
    pub connection_timeouts: MgmtConnectionTimeouts,
    pub request_timeouts: MgmtRequestTimeouts,
    pub overload: MgmtOverloadOptions,
    pub emit_evidence_log: bool,
}

impl MgmtTransportProfileV1 {
    #[inline]
    pub fn normalized(mut self) -> Self {
        self.shutdown_timeout = self.shutdown_timeout.max(Duration::from_millis(1));
        self.http = self.http.normalized();
        self.isolation = self.isolation.normalized();
        self.overload = self.overload.normalized();
        self
    }

    /// Build a transport `DataPlaneOptions` from this profile.
    #[inline]
    pub fn transport_options(&self) -> DataPlaneOptions {
        let p = self.clone().normalized();
        let max_req = p.http.effective_max_request_bytes();

        DataPlaneOptions::management_http_with_limits(
            p.bind,
            max_req,
            p.http.max_head_bytes,
            p.http.max_headers,
        )
        .with_max_channels(p.isolation.limits.max_channels)
        .with_max_accept_per_tick(p.isolation.limits.max_accept_per_tick)
        .with_budget(p.isolation.budget)
        .with_drain_timeout(p.shutdown_timeout)
        .with_evidence_log(p.emit_evidence_log)
    }

    /// Build a transport `DataPlaneConfig` from this profile.
    #[inline]
    pub fn transport_config(&self) -> DataPlaneConfig {
        self.transport_options().build()
    }

    /// Build a stable, runtime-auditable mgmt profile effective snapshot.
    #[inline]
    pub fn describe_effective_config(&self) -> MgmtProfileEffectiveConfig {
        let normalized = self.clone().normalized();
        MgmtProfileEffectiveConfig {
            bind: normalized.bind,
            shutdown_timeout: normalized.shutdown_timeout,
            http: normalized.http,
            effective_max_request_bytes: normalized.http.effective_max_request_bytes(),
            isolation: normalized.isolation,
            connection_timeouts: normalized.connection_timeouts,
            request_timeouts: normalized.request_timeouts,
            overload: normalized.overload,
            emit_evidence_log: normalized.emit_evidence_log,
        }
    }

    /// Build a throughput-oriented management transport profile.
    ///
    /// This keeps the bind/framing/limits intact, while applying a conservative perf overlay.
    #[inline]
    pub fn transport_perf_config(&self) -> DataPlaneConfig {
        self.transport_options().with_perf_defaults().build()
    }
}

impl Default for MgmtTransportProfileV1 {
    fn default() -> Self {
        Self {
            bind: SocketAddr::from(([127, 0, 0, 1], 8080)),
            shutdown_timeout: Duration::from_secs(10),
            http: MgmtHttpLimits::default(),
            isolation: MgmtIsolationOptions::default(),
            connection_timeouts: MgmtConnectionTimeouts::default(),
            request_timeouts: MgmtRequestTimeouts::default(),
            overload: MgmtOverloadOptions::default(),
            emit_evidence_log: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MgmtConnectionTimeouts, MgmtHttpLimits, MgmtIsolationOptions, MgmtOverloadOptions,
        MgmtRequestTimeouts, MgmtTransportProfileV1,
    };
    use spark_transport::async_bridge::FrameDecoderProfile;
    use spark_transport::{Budget, DataPlaneLimits};
    use std::time::Duration;

    #[test]
    fn mgmt_http_limits_effective_max_covers_head_plus_body() {
        let l = MgmtHttpLimits {
            max_head_bytes: 4096,
            max_headers: 8,
            max_body_bytes: 10 * 1024,
            max_request_bytes: 4096,
        };
        assert!(l.effective_max_request_bytes() >= 4096 + 10 * 1024);
    }

    #[test]
    fn mgmt_profile_transport_config_tracks_limits_and_budget() {
        let p = MgmtTransportProfileV1 {
            bind: "127.0.0.1:0".parse().expect("addr"),
            shutdown_timeout: Duration::from_secs(2),
            http: MgmtHttpLimits {
                max_head_bytes: 2048,
                max_headers: 4,
                max_body_bytes: 4096,
                max_request_bytes: 4096,
            },
            isolation: MgmtIsolationOptions {
                max_inflight: 3,
                limits: DataPlaneLimits {
                    max_channels: 7,
                    max_accept_per_tick: 5,
                },
                budget: Budget {
                    max_events: 64,
                    max_nanos: 500_000,
                },
            },
            connection_timeouts: MgmtConnectionTimeouts::default(),
            request_timeouts: MgmtRequestTimeouts::default(),
            overload: MgmtOverloadOptions::default(),
            emit_evidence_log: false,
        };

        let dp = p.transport_config();
        assert_eq!(dp.bind, p.bind);
        assert_eq!(dp.max_channels, 7);
        assert_eq!(dp.max_accept_per_tick, 5);
        assert_eq!(dp.drain_timeout, p.shutdown_timeout);
        assert_eq!(dp.budget.max_events, 64);

        match dp.framing {
            FrameDecoderProfile::Http1 {
                max_request_bytes,
                max_head_bytes,
                max_headers,
            } => {
                assert_eq!(max_request_bytes, p.http.effective_max_request_bytes());
                assert_eq!(
                    max_head_bytes,
                    p.http
                        .max_head_bytes
                        .min(p.http.effective_max_request_bytes())
                );
                assert_eq!(max_headers, p.http.max_headers.max(1));
            }
            other => panic!("expected Http1 framing, got {other:?}"),
        }
    }

    #[test]
    fn mgmt_profile_describe_effective_config_is_normalized() {
        let p = MgmtTransportProfileV1 {
            bind: "127.0.0.1:0".parse().expect("addr"),
            shutdown_timeout: Duration::ZERO,
            http: MgmtHttpLimits {
                max_head_bytes: 0,
                max_headers: 0,
                max_body_bytes: 1024,
                max_request_bytes: 0,
            },
            isolation: MgmtIsolationOptions {
                max_inflight: 0,
                limits: DataPlaneLimits {
                    max_channels: 0,
                    max_accept_per_tick: 0,
                },
                budget: Budget {
                    max_events: 5,
                    max_nanos: 6,
                },
            },
            connection_timeouts: MgmtConnectionTimeouts::default(),
            request_timeouts: MgmtRequestTimeouts::default(),
            overload: MgmtOverloadOptions::default(),
            emit_evidence_log: true,
        };

        let effective = p.describe_effective_config();
        assert_eq!(effective.shutdown_timeout, Duration::from_millis(1));
        assert_eq!(effective.http.max_head_bytes, 1);
        assert_eq!(effective.http.max_headers, 1);
        assert_eq!(effective.http.max_request_bytes, 1);
        assert_eq!(effective.effective_max_request_bytes, 1025);
        assert_eq!(effective.isolation.max_inflight, 1);
        assert_eq!(effective.isolation.limits.max_channels, 1);
        assert_eq!(effective.isolation.limits.max_accept_per_tick, 1);
        assert!(effective.emit_evidence_log);
    }
}
