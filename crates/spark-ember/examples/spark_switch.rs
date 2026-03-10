//! `spark-switch`：最小可运行示例（Windows 友好）。
//!
//! 说明：
//! - 在 Rust 里，示例通常放在 `examples/`，不会影响主干库的编译与发布。
//! - 运行方式：`cargo run -p spark-ember --example spark_switch --features transport-mgmt,backend-mio`
//!   （或 `backend-iocp`）
//! - DECISION（Backend-neutral Ember）：`spark-ember` 本体不绑定任何 IO 后端，
//!   后端由上层分发（distribution）或示例通过 feature 注入。
//! - 退出方式：在另一个终端执行 `curl -X POST http://127.0.0.1:8080/drain`，触发优雅退出，
//!   避免 Windows 下 Ctrl+C 的 `0xc000013a` 误解。

// DECISION (Backend-neutral Ember): this example must compile under `cargo clippy --all-targets -D warnings`
// even when no backend feature is enabled. Gate backend-specific imports to avoid unused warnings.

use spark_buffer::Bytes;
use spark_core::context::Context;
use spark_core::router::{Router2, Select2};
use spark_core::service::Service;
use spark_transport::KernelError;

use spark_ember::run_blocking;
use spark_host::{HostBuilder, ServerConfig};

/// 回显服务：收到任何请求都返回固定 payload。
#[derive(Clone)]
struct PongService {
    payload: Bytes,
}

impl PongService {
    fn new(size: usize) -> Self {
        let mut v = Vec::with_capacity(size.max(5));
        v.extend_from_slice(b"PONG\n");
        if size > v.len() {
            v.resize(size, b'P');
        }
        Self { payload: Bytes::from(v) }
    }
}

impl Service<Bytes> for PongService {
    type Response = Option<Bytes>;
    type Error = KernelError;

    async fn call(&self, _context: Context, _request: Bytes) -> Result<Self::Response, Self::Error> {
        Ok(Some(self.payload.clone()))
    }
}

#[derive(Clone)]
struct DropService;

impl Service<Bytes> for DropService {
    type Response = Option<Bytes>;
    type Error = KernelError;

    async fn call(&self, _context: Context, _request: Bytes) -> Result<Self::Response, Self::Error> {
        Ok(None)
    }
}

/// 路由选择器：如果请求以 "PING" 开头则走 pong，否则 drop。
#[derive(Clone)]
struct PingSelector;

impl Select2<Bytes> for PingSelector {
    fn select(&self, _ctx: &Context, req: &Bytes) -> bool {
        req.starts_with(b"PING")
    }
}

fn build_router(big_size: usize) -> Router2<PingSelector, PongService, DropService> {
    Router2::new(PingSelector, PongService::new(big_size), DropService, "pong", "drop")
}

fn main() -> std::io::Result<()> {
    // 1MiB big PONG to trigger write backpressure.
    let app = build_router(1024 * 1024);

    let spec = HostBuilder::new()
        .config(ServerConfig { name: "spark-switch", ..Default::default() })
        .use_default_diagnostics()
        .pipeline(|p| p.service(app))
        .build().map_err(|_e| std::io::Error::new(std::io::ErrorKind::InvalidInput, "HostBuilder build failed"))?;

    run_blocking(
        // DECISION (clippy-clean examples): closure parameters are prefixed with `_` so the example
        // stays `-D warnings` clean in all feature combinations (backend enabled/disabled).
        spec,
        |_spec, _draining| {
            // DECISION: examples must compile without a hard backend dependency.
            // When no backend feature is enabled, we fail fast with a clear message.
            #[cfg(feature = "backend-mio")]
            {
                spark_transport_mio::spawn_tcp_dataplane(
                    _spec.dataplane.clone(),
                    draining,
                    std::sync::std::sync::Arc::clone(&_spec.service),
                    std::sync::std::sync::Arc::clone(&_spec.metrics),
                )
                .map(|h| h.join)
            }

            #[cfg(feature = "backend-iocp")]
            {
                spark_transport_iocp::spawn_tcp_dataplane(
                    _spec.dataplane.clone(),
                    draining,
                    std::sync::std::sync::Arc::clone(&_spec.service),
                    std::sync::std::sync::Arc::clone(&_spec.metrics),
                )
                .map(|h| h.join)
            }

            #[cfg(not(any(feature = "backend-mio", feature = "backend-iocp")))]
            {
                // DECISION: keep parameter names stable for readability when a backend is enabled;
                // when no backend is enabled, mark them as used to satisfy `-D warnings`.
                let _ = (&_spec, &_draining);
                Err(std::io::Error::other(
                    "spark_switch: enable a backend feature: --features transport-mgmt,backend-mio (or backend-iocp)",
                ))
            }
        },
        |_cfg, _draining, _service, _metrics| {
            // DECISION: mgmt-plane must ride the same backend as the distribution (or example selection).
            #[cfg(feature = "backend-mio")]
            {
                spark_transport_mio::try_spawn_tcp_dataplane(cfg, _draining, service, metrics).map(|h| {
                    spark_ember::http1::SpawnedTcpDataplane {
                        join: h.join,
                        local_addr: h.local_addr,
                    }
                })
            }

            #[cfg(feature = "backend-iocp")]
            {
                spark_transport_iocp::try_spawn_tcp_dataplane(cfg, _draining, service, metrics).map(|h| {
                    spark_ember::http1::SpawnedTcpDataplane {
                        join: h.join,
                        local_addr: h.local_addr,
                    }
                })
            }

            #[cfg(not(any(feature = "backend-mio", feature = "backend-iocp")))]
            {
                // DECISION: same as above; keep the signature meaningful for backend-enabled builds.
                let _ = (&_cfg, &_draining, &_service, &_metrics);
                Err(std::io::Error::other(
                    "spark_switch: enable a backend feature for mgmt-plane: --features transport-mgmt,backend-mio (or backend-iocp)",
                ))
            }
        },
    )
}
