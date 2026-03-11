//! HTTP/1.1 management-plane server implemented on top of `spark-transport` (backend-injected).
//!
//! This is a "dogfooding" profile:
//! - It exercises Spark's transport pipeline, backpressure, evidence, and fairness.
//! - It coexists with the std-only `Server` (thread-per-conn) implementation.
//!
//! Notes:
//! - Management QPS is low; correctness, observability, and clean layering matter more than micro-optimizations.
//! - The transport framing profile emits **complete requests** (head + body bytes).

use std::net::SocketAddr;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

use spark_buffer::Bytes;
use spark_codec::prelude::*;
use spark_codec_http::http1::{write_response, Http1DecodeError, Http1HeadDecoder};
use spark_core::context::Context;
use spark_core::service::Service;
use spark_host::mgmt_profile::MgmtTransportProfileV1;
use spark_host::router::{MgmtRequest, MgmtResponse, MgmtState, RouteKind, RouteTable};
use spark_transport::async_bridge::OverloadAction;
use spark_transport::{DataPlaneConfig, DataPlaneMetrics, KernelError};
use std::time::Instant;

use super::EmberState;
use crate::util::block_on_until;

/// Management-plane server over Spark transport.
#[derive(Debug)]
pub struct TransportServer {
    profile: MgmtTransportProfileV1,
    routes: Arc<RouteTable>,
    state: Arc<EmberState>,
    metrics: Arc<DataPlaneMetrics>,
}

/// A spawned TCP dataplane used by the management server.
///
/// DECISION (BigStep-11): `spark-ember` must not depend on any concrete backend crate.
/// Instead, the distribution layer injects a spawn function that returns this minimal handle.
#[derive(Debug)]
pub struct SpawnedTcpDataplane {
    pub join: thread::JoinHandle<()>,
    pub local_addr: SocketAddr,
}

/// A spawned transport-backed management server.
///
/// The `addr` field is particularly important for:
/// - dogfooding smoke tests (bind to port 0 and connect to the actual chosen port);
/// - embedding scenarios where port selection is delegated to the OS.
#[derive(Debug)]
pub struct TransportServerHandle {
    pub join: thread::JoinHandle<()>,
    pub addr: SocketAddr,
    pub state: Arc<EmberState>,
}

impl TransportServer {
    pub fn new(
        profile: MgmtTransportProfileV1,
        routes: Arc<RouteTable>,
        state: Arc<EmberState>,
        metrics: Arc<DataPlaneMetrics>,
    ) -> Self {
        Self {
            profile,
            routes,
            state,
            metrics,
        }
    }

    #[inline]
    pub fn state(&self) -> Arc<EmberState> {
        Arc::clone(&self.state)
    }

    /// Spawn the management server using an injected backend spawn function.
    ///
    /// DECISION (dogfooding gate): mgmt-plane must run on the *same* transport stack as the dataplane
    /// for each distribution. Therefore `spark-ember` receives a backend-specific spawn closure.
    pub fn try_spawn_with<Spawn>(self, spawn: Spawn) -> std::io::Result<TransportServerHandle>
    where
        Spawn: FnOnce(
            DataPlaneConfig,
            Arc<AtomicBool>,
            Arc<HttpMgmtService>,
            Arc<DataPlaneMetrics>,
        ) -> std::io::Result<SpawnedTcpDataplane>,
    {
        let cfg = self.profile.transport_config();
        self.try_spawn_with_config(cfg, spawn)
    }

    /// Spawn the management server with the throughput-oriented transport profile.
    pub fn try_spawn_perf_with<Spawn>(self, spawn: Spawn) -> std::io::Result<TransportServerHandle>
    where
        Spawn: FnOnce(
            DataPlaneConfig,
            Arc<AtomicBool>,
            Arc<HttpMgmtService>,
            Arc<DataPlaneMetrics>,
        ) -> std::io::Result<SpawnedTcpDataplane>,
    {
        let cfg = self.profile.transport_perf_config();
        self.try_spawn_with_config(cfg, spawn)
    }

    fn try_spawn_with_config<Spawn>(
        self,
        cfg: DataPlaneConfig,
        spawn: Spawn,
    ) -> std::io::Result<TransportServerHandle>
    where
        Spawn: FnOnce(
            DataPlaneConfig,
            Arc<AtomicBool>,
            Arc<HttpMgmtService>,
            Arc<DataPlaneMetrics>,
        ) -> std::io::Result<SpawnedTcpDataplane>,
    {
        let cfg = cfg
            .with_max_inflight_per_connection(self.profile.overload.max_inflight_per_connection)
            .with_max_queue_per_connection(self.profile.overload.queue_limit)
            .with_overload_action(overload_action(self.profile.overload.reject_policy));

        let service = Arc::new(HttpMgmtService {
            routes: Arc::clone(&self.routes),
            state: self.state() as Arc<dyn MgmtState>,
            max_request_bytes: self.profile.http.effective_max_request_bytes(),
            max_head_bytes: self.profile.http.max_head_bytes,
            max_headers: self.profile.http.max_headers,
            max_body_bytes: self.profile.http.max_body_bytes,
            default_request_timeout: self.profile.request_timeouts.default_timeout,
            max_inflight: self.profile.overload.max_concurrent_requests,
            queue_limit: self.profile.overload.queue_limit,
            max_inflight_per_connection: self.profile.overload.max_inflight_per_connection,
            reject_policy: self.profile.overload.reject_policy,
            inflight: AtomicUsize::new(0),
            queued: AtomicUsize::new(0),
        });

        let draining = self.state.draining_handle();
        let metrics = Arc::clone(&self.metrics);

        let dp = spawn(cfg, draining, service, metrics)?;
        Ok(TransportServerHandle {
            join: dp.join,
            addr: dp.local_addr,
            state: self.state(),
        })
    }
}

pub struct HttpMgmtService {
    routes: Arc<RouteTable>,
    state: Arc<dyn MgmtState>,
    max_request_bytes: usize,
    max_head_bytes: usize,
    max_headers: usize,
    max_body_bytes: usize,
    default_request_timeout: std::time::Duration,
    max_inflight: usize,
    queue_limit: usize,
    max_inflight_per_connection: usize,
    reject_policy: spark_host::mgmt_profile::MgmtRejectPolicy,
    inflight: AtomicUsize,
    queued: AtomicUsize,
}

// Keep Debug minimal: do not require MgmtState: Debug.
impl std::fmt::Debug for HttpMgmtService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpMgmtService")
            .field("routes", &"<RouteTable>")
            .field("state", &"<MgmtState>")
            .field("max_request_bytes", &self.max_request_bytes)
            .field("max_head_bytes", &self.max_head_bytes)
            .field("max_headers", &self.max_headers)
            .field("max_body_bytes", &self.max_body_bytes)
            .field("default_request_timeout", &self.default_request_timeout)
            .field("max_inflight", &self.max_inflight)
            .field("queue_limit", &self.queue_limit)
            .field(
                "max_inflight_per_connection",
                &self.max_inflight_per_connection,
            )
            .field("reject_policy", &self.reject_policy)
            .finish()
    }
}

struct InflightGuard<'a> {
    inflight: &'a AtomicUsize,
}

impl Drop for InflightGuard<'_> {
    fn drop(&mut self) {
        self.inflight.fetch_sub(1, Ordering::Release);
    }
}

impl Service<Bytes> for HttpMgmtService {
    type Response = Option<Bytes>;
    type Error = KernelError;

    async fn call(
        &self,
        mut context: Context,
        request: Bytes,
    ) -> Result<Self::Response, Self::Error> {
        if self.state.is_draining() {
            return Ok(Some(resp_draining()));
        }

        let queued_prev = self.queued.fetch_add(1, Ordering::AcqRel);
        if queued_prev >= self.queue_limit {
            self.queued.fetch_sub(1, Ordering::Release);
            return Ok(reject_overload(self.reject_policy));
        }

        // Simple concurrency cap to keep mgmt isolated from pathological clients.
        let prev = self.inflight.fetch_add(1, Ordering::AcqRel);
        if prev >= self.max_inflight {
            self.inflight.fetch_sub(1, Ordering::Release);
            self.queued.fetch_sub(1, Ordering::Release);
            return Ok(reject_overload(self.reject_policy));
        }
        self.queued.fetch_sub(1, Ordering::Release);
        let _guard = InflightGuard {
            inflight: &self.inflight,
        };

        if request.len() > self.max_request_bytes {
            return Ok(Some(resp_413()));
        }

        let mut decoder = Http1HeadDecoder::with_limits(self.max_head_bytes, self.max_headers);
        let (consumed, req) = match decoder.decode(&mut context, request.as_ref()) {
            Ok(DecodeOutcome::Message { consumed, message }) => {
                let content_len = message.content_length();
                if content_len > self.max_body_bytes {
                    return Ok(Some(resp_413()));
                }
                let need = consumed.saturating_add(content_len);
                if need > self.max_request_bytes {
                    return Ok(Some(resp_413()));
                }
                if request.len() < need {
                    return Ok(Some(resp_400()));
                }
                (consumed, message)
            }
            Ok(DecodeOutcome::NeedMore) => return Ok(Some(resp_400())),
            Err(Http1DecodeError::HeadTooLarge) => return Ok(Some(resp_413())),
            Err(_) => return Ok(Some(resp_400())),
        };

        let content_len = req.content_length();
        let body = &request.as_ref()[consumed..consumed.saturating_add(content_len)];

        let kind_ref = req.method.as_ref();
        let path_ref = req.path.as_ref();

        let resp = if let Some(entry) = self.routes.lookup(kind_ref, path_ref) {
            let timeout = entry.effective_request_timeout(self.default_request_timeout);
            let deadline = Instant::now() + timeout;
            match block_on_until(
                (entry.handler)(MgmtRequest {
                    kind: RouteKind::from(kind_ref),
                    path: req.path,
                    body: body.to_vec(),
                    state: Arc::clone(&self.state),
                }),
                deadline,
            ) {
                Some(resp) => resp,
                None => MgmtResponse::status(504, "Request Timeout"),
            }
        } else {
            MgmtResponse::status(404, "Not Found")
        };

        Ok(Some(encode_resp(
            resp.status,
            resp.content_type,
            &resp.body,
        )))
    }
}

#[inline]
fn resp_400() -> Bytes {
    encode_resp(400, "text/plain; charset=utf-8", b"Bad Request")
}

#[inline]
fn resp_413() -> Bytes {
    encode_resp(413, "text/plain; charset=utf-8", b"Request Too Large")
}

#[inline]
fn resp_draining() -> Bytes {
    encode_resp(503, "text/plain; charset=utf-8", b"Draining")
}

#[inline]
fn reject_overload(policy: spark_host::mgmt_profile::MgmtRejectPolicy) -> Option<Bytes> {
    match policy {
        spark_host::mgmt_profile::MgmtRejectPolicy::ServiceUnavailable => {
            Some(encode_resp(503, "text/plain; charset=utf-8", b"Busy"))
        }
        spark_host::mgmt_profile::MgmtRejectPolicy::TooManyRequests => {
            Some(encode_resp(429, "text/plain; charset=utf-8", b"Busy"))
        }
        // The transport overload policy can close the connection directly.
        // If the request reached this service anyway, return a 503 fallback.
        spark_host::mgmt_profile::MgmtRejectPolicy::CloseConnection => {
            Some(encode_resp(503, "text/plain; charset=utf-8", b"Busy"))
        }
    }
}

#[inline]
fn overload_action(policy: spark_host::mgmt_profile::MgmtRejectPolicy) -> OverloadAction {
    match policy {
        spark_host::mgmt_profile::MgmtRejectPolicy::ServiceUnavailable => OverloadAction::FailFast,
        spark_host::mgmt_profile::MgmtRejectPolicy::TooManyRequests => OverloadAction::FailFast,
        spark_host::mgmt_profile::MgmtRejectPolicy::CloseConnection => {
            OverloadAction::CloseConnection
        }
    }
}

fn encode_resp(status: u16, content_type: &str, body: &[u8]) -> Bytes {
    let mut out = Vec::<u8>::with_capacity(body.len().saturating_add(128));
    // Writing to a Vec never fails.
    let _ = write_response(&mut out, status, content_type, body);
    Bytes::from(out)
}

#[cfg(test)]
mod tests {
    use super::HttpMgmtService;
    use crate::http1::EmberState;
    use crate::util::block_on;
    use spark_buffer::Bytes;
    use spark_core::context::Context;
    use spark_core::service::Service;
    use spark_host::mgmt_profile::MgmtRejectPolicy;
    use spark_host::router::{MgmtApp, MgmtResponse, RouteTable};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    fn build_service(
        app: MgmtApp,
        state: Arc<EmberState>,
        default_request_timeout: Duration,
        max_inflight: usize,
        queue_limit: usize,
        reject_policy: MgmtRejectPolicy,
    ) -> HttpMgmtService {
        let routes = Arc::new(RouteTable::new());
        routes.replace_all(app.into_routes());
        HttpMgmtService {
            routes,
            state,
            max_request_bytes: 16 * 1024,
            max_head_bytes: 8 * 1024,
            max_headers: 32,
            max_body_bytes: 8 * 1024,
            default_request_timeout,
            max_inflight,
            queue_limit,
            max_inflight_per_connection: 1,
            reject_policy,
            inflight: AtomicUsize::new(0),
            queued: AtomicUsize::new(0),
        }
    }

    fn make_get(path: &str) -> Bytes {
        Bytes::from(format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").into_bytes())
    }

    fn resp_status(resp: &Bytes) -> Option<u16> {
        let data = resp.as_ref();
        let line_end = data.windows(2).position(|w| w == b"\r\n")?;
        let line = core::str::from_utf8(&data[..line_end]).ok()?;
        let mut parts = line.split_whitespace();
        let _ = parts.next()?;
        parts.next()?.parse::<u16>().ok()
    }

    #[test]
    fn transport_service_route_timeout_returns_504() {
        let mut app = MgmtApp::new();
        app.map_get("/slow", |_req| async {
            core::future::pending::<MgmtResponse>().await
        });
        let state = Arc::new(EmberState::new());
        let svc = build_service(
            app,
            state,
            Duration::from_millis(20),
            4,
            4,
            MgmtRejectPolicy::ServiceUnavailable,
        );

        let out = block_on(svc.call(Context::default(), make_get("/slow")));
        let bytes = out
            .ok()
            .flatten()
            .unwrap_or_else(|| Bytes::from_static(b""));
        assert_eq!(resp_status(&bytes), Some(504));
    }

    #[test]
    fn transport_service_group_default_timeout_returns_504() {
        let mut app = MgmtApp::new();
        let mut group = app.map_group("/g");
        group.with_request_timeout(Duration::from_millis(10));
        group.map_get("/slow", |_req| async {
            core::future::pending::<MgmtResponse>().await
        });
        let state = Arc::new(EmberState::new());
        let svc = build_service(
            app,
            state,
            Duration::from_secs(1),
            4,
            4,
            MgmtRejectPolicy::ServiceUnavailable,
        );

        let out = block_on(svc.call(Context::default(), make_get("/g/slow")));
        let bytes = out
            .ok()
            .flatten()
            .unwrap_or_else(|| Bytes::from_static(b""));
        assert_eq!(resp_status(&bytes), Some(504));
    }

    #[test]
    fn transport_service_draining_rejects() {
        let mut app = MgmtApp::new();
        app.map_get("/healthz", |_req| async { MgmtResponse::ok("OK") });
        let state = Arc::new(EmberState::new());
        state.set_draining(true);
        let svc = build_service(
            app,
            state,
            Duration::from_secs(1),
            4,
            4,
            MgmtRejectPolicy::ServiceUnavailable,
        );

        let out = block_on(svc.call(Context::default(), make_get("/healthz")));
        let bytes = out
            .ok()
            .flatten()
            .unwrap_or_else(|| Bytes::from_static(b""));
        assert_eq!(resp_status(&bytes), Some(503));
    }

    #[test]
    fn transport_service_overload_rejects_with_policy() {
        let mut app = MgmtApp::new();
        app.map_get("/ok", |_req| async { MgmtResponse::ok("OK") });
        let state = Arc::new(EmberState::new());
        let svc = build_service(
            app,
            state,
            Duration::from_secs(1),
            1,
            4,
            MgmtRejectPolicy::TooManyRequests,
        );
        svc.inflight.store(1, Ordering::Release);

        let out = block_on(svc.call(Context::default(), make_get("/ok")));
        let bytes = out
            .ok()
            .flatten()
            .unwrap_or_else(|| Bytes::from_static(b""));
        assert_eq!(resp_status(&bytes), Some(429));
    }
}
