use spark_buffer::Bytes;

use spark_core::pipeline::{IdentityLayer, PipelineBuilder};
use spark_core::service::Service;
use spark_transport::KernelError;
use spark_transport::{DataPlaneConfig, DataPlaneMetrics, DataPlaneOptions};

use crate::config::ServerConfig;
use crate::router::{MgmtApp, MgmtGroup, MgmtRequest, MgmtResponse, RouteKind, RouteSet};
use crate::RouteMetrics;

use std::future::Future;
use std::sync::Arc;
use std::time::Instant;

pub struct NoPipeline;

/// `HostBuilder` 构建出的运行时中立描述（HostSpec）。
///
/// 关键点：
/// - HostSpec 只是“装配结果”的数据载体，不包含 listen/accept/事件循环；
/// - 运行（run）由上层应用或 server 发行物（如 `spark-ember`）决定；
/// - 这样才能做到：不绑死 Tokio/mio/io_uring，且依赖不会向上传染。
pub struct HostSpec<S> {
    pub config: ServerConfig,
    pub mgmt: RouteSet,
    pub dataplane: DataPlaneConfig,
    pub metrics: Arc<DataPlaneMetrics>,
    pub route_metrics: Arc<RouteMetrics>,
    pub service: Arc<S>,
}

pub struct HostBuilder<S> {
    config: ServerConfig,
    mgmt: MgmtApp,
    dataplane: DataPlaneConfig,
    metrics: Arc<DataPlaneMetrics>,
    route_metrics: Arc<RouteMetrics>,
    service: Option<Arc<S>>,
}

impl HostBuilder<NoPipeline> {
    pub fn new() -> Self {
        Self {
            config: ServerConfig::default(),
            mgmt: MgmtApp::new(),
            dataplane: DataPlaneConfig::default(),
            metrics: Arc::new(DataPlaneMetrics::default()),
            route_metrics: Arc::new(RouteMetrics::new()),
            service: None,
        }
    }

    pub fn pipeline<F, Svc>(self, build: F) -> HostBuilder<Svc>
    where
        F: FnOnce(PipelineBuilder<IdentityLayer>) -> Svc,
        Svc: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
    {
        let svc = build(PipelineBuilder::<IdentityLayer>::new());
        HostBuilder {
            config: self.config,
            mgmt: self.mgmt,
            dataplane: self.dataplane,
            metrics: self.metrics,
            route_metrics: self.route_metrics,
            service: Some(Arc::new(svc)),
        }
    }
}

impl Default for HostBuilder<NoPipeline> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<S> HostBuilder<S> {
    pub fn config(mut self, config: ServerConfig) -> Self {
        self.config = config;
        self
    }

    pub fn dataplane(mut self, cfg: DataPlaneConfig) -> Self {
        self.dataplane = cfg;
        self
    }

    /// Configure the dataplane via the higher-level Options façade.
    pub fn dataplane_options(mut self, options: DataPlaneOptions) -> Self {
        self.dataplane = options.build();
        self
    }

    /// Apply a sane transport-backed management profile.
    ///
    /// Call this after `config(...)` if you want the dataplane bind/max-request settings to follow
    /// the current management config values.
    pub fn use_transport_mgmt_profile(mut self) -> Self {
        self.dataplane = self.config.management_transport_config();
        self
    }

    /// Apply a throughput-oriented tuning overlay to the current dataplane shape.
    ///
    /// This keeps the existing bind/framing/limits and only adjusts runtime budgets and
    /// backpressure thresholds for higher-throughput bring-up.
    pub fn use_transport_perf_profile(mut self) -> Self {
        self.dataplane = self.dataplane.with_perf_defaults();
        self
    }

    /// Apply the canonical transport-backed management profile with throughput-oriented tuning.
    pub fn use_transport_mgmt_perf_profile(mut self) -> Self {
        self.dataplane = self.config.management_transport_perf_config();
        self
    }

    /// Configure mgmt routing (control-plane) using a minimal-API style surface.
    pub fn mgmt(mut self, f: impl FnOnce(&mut MgmtApp)) -> Self {
        f(&mut self.mgmt);
        self
    }

    /// Configure a group of routes with a shared prefix (ASP.NET Core `MapGroup` inspired).
    pub fn map_group(mut self, prefix: &'static str, f: impl FnOnce(&mut MgmtGroup<'_>)) -> Self {
        let mut group = self.mgmt.map_group(prefix);
        f(&mut group);
        self
    }
    pub fn map_get<F, Fut>(
        self,
        path: impl Into<Box<str>>,
        route_id: impl Into<Box<str>>,
        handler: F,
    ) -> Self
    where
        F: Fn(MgmtRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = MgmtResponse> + Send + 'static,
    {
        let path = path.into();
        let route_id: Box<str> = route_id.into();

        // Register a stable per-route counter once; request paths stay low-cardinality.
        let counter = self.route_metrics.register(route_id.clone());
        let h = Arc::new(handler);

        self.mgmt(|app| {
            let counter = Arc::clone(&counter);
            let h = Arc::clone(&h);
            let wrapped = move |req: MgmtRequest| {
                let g = counter.begin();
                let start = Instant::now();
                let h = Arc::clone(&h);
                async move {
                    let resp = (h)(req).await;
                    g.finish(resp.status, start.elapsed());
                    resp
                }
            };
            app.map_get(path, wrapped).named(route_id);
        })
    }

    /// 注册一个 POST 路由（控制面）。
    ///
    /// 给不熟 Rust/OOP 的同学提示：
    /// - 这里不是“继承 Controller”，而是把 handler 作为函数值注册到路由表；
    /// - handler 的类型在编译期确定（零开销），RouteTable 只存一个 `Arc<dyn Fn>`。
    pub fn map_post<F, Fut>(
        self,
        path: impl Into<Box<str>>,
        route_id: impl Into<Box<str>>,
        handler: F,
    ) -> Self
    where
        F: Fn(MgmtRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = MgmtResponse> + Send + 'static,
    {
        let path = path.into();
        let route_id: Box<str> = route_id.into();

        let counter = self.route_metrics.register(route_id.clone());
        let h = Arc::new(handler);

        self.mgmt(|app| {
            let counter = Arc::clone(&counter);
            let h = Arc::clone(&h);
            let wrapped = move |req: MgmtRequest| {
                let g = counter.begin();
                let start = Instant::now();
                let h = Arc::clone(&h);
                async move {
                    let resp = (h)(req).await;
                    g.finish(resp.status, start.elapsed());
                    resp
                }
            };
            app.map_post(path, wrapped).named(route_id);
        })
    }

    /// Register a control-plane route by `(kind, path)`.
    ///
    /// This keeps routing **protocol-agnostic**: HTTP adapters use kind="GET"/"POST" etc,
    /// while other adapters may use custom kinds.
    pub fn map_kind<K, F, Fut>(
        self,
        kind: K,
        path: impl Into<Box<str>>,
        route_id: impl Into<Box<str>>,
        handler: F,
    ) -> Self
    where
        K: Into<RouteKind>,
        F: Fn(MgmtRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = MgmtResponse> + Send + 'static,
    {
        let kind: RouteKind = kind.into();
        let path = path.into();
        let route_id: Box<str> = route_id.into();

        let counter = self.route_metrics.register(route_id.clone());
        let h = Arc::new(handler);

        self.mgmt(|app| {
            let counter = Arc::clone(&counter);
            let h = Arc::clone(&h);
            let wrapped = move |req: MgmtRequest| {
                let g = counter.begin();
                let start = Instant::now();
                let h = Arc::clone(&h);
                async move {
                    let resp = (h)(req).await;
                    g.finish(resp.status, start.elapsed());
                    resp
                }
            };
            app.map(kind, path, wrapped).named(route_id);
        })
    }

    pub fn use_default_diagnostics(mut self) -> Self {
        // /healthz
        self = self.map_get("/healthz", "healthz", |_| async { MgmtResponse::ok("OK") });

        // /readyz
        self = self.map_get("/readyz", "readyz", |req| async move {
            if req.state.is_draining() {
                return MgmtResponse::status(503, "Draining");
            }
            MgmtResponse::ok("Ready")
        });

        // /metrics
        let m = Arc::clone(&self.metrics);
        let rm = Arc::clone(&self.route_metrics);
        self = self.map_get("/metrics", "metrics", move |_req| {
            let m = Arc::clone(&m);
            let rm = Arc::clone(&rm);
            async move {
                let mut text = spark_metrics_prometheus::render_prometheus(&m);
                text.push_str(&rm.render_prometheus());
                MgmtResponse {
                    status: 200,
                    content_type: "text/plain; version=0.0.4; charset=utf-8",
                    body: text.into_bytes(),
                }
            }
        });

        // /drain (POST)
        // 通过 MgmtState::request_draining() 请求优雅退出。
        // 是否真正支持由具体 server/state 决定（默认 no-op）。
        self = self.map_post("/drain", "drain", |req| async move {
            req.state.request_draining();
            MgmtResponse::ok("Draining")
        });

        self
    }
}

impl<S> HostBuilder<S>
where
    S: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
{
    /// Build a runtime-neutral spec.
    ///
    /// This never panics. If the pipeline was not configured, it returns `KernelError::Invalid`.
    pub fn build(self) -> core::result::Result<HostSpec<S>, KernelError> {
        let service = match self.service {
            Some(s) => s,
            None => return Err(KernelError::Invalid),
        };

        Ok(HostSpec {
            config: self.config,
            mgmt: self.mgmt.into_routes(),
            dataplane: self.dataplane,
            metrics: self.metrics,
            route_metrics: self.route_metrics,
            service,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{HostBuilder, NoPipeline};
    use crate::config::ServerConfig;
    use spark_transport::DataPlaneConfig;

    #[test]
    fn transport_perf_profile_preserves_bind_and_applies_overlay() {
        let bind = "127.0.0.1:25070".parse().expect("static addr");
        let builder = HostBuilder::<NoPipeline>::new()
            .dataplane(DataPlaneConfig::tcp(bind).with_evidence_log(true))
            .use_transport_perf_profile();

        assert_eq!(builder.dataplane.bind, bind);
        assert!(!builder.dataplane.emit_evidence_log);
        assert_eq!(builder.dataplane.flush_policy.max_syscalls, 64);
        assert_eq!(builder.dataplane.watermark.low_mul, 8);
        assert_eq!(builder.dataplane.budget.max_events, 512);
    }

    #[test]
    fn transport_mgmt_perf_profile_tracks_server_config() {
        let cfg = ServerConfig::default()
            .with_mgmt_addr("127.0.0.1:25071".parse().expect("static addr"))
            .with_max_request_bytes(32 * 1024);
        let builder = HostBuilder::<NoPipeline>::new()
            .config(cfg.clone())
            .use_transport_mgmt_perf_profile();

        assert_eq!(builder.dataplane.bind, cfg.mgmt.bind);
        assert_eq!(
            builder.dataplane.max_frame_hint(),
            cfg.effective_max_request_bytes()
        );
        assert_eq!(builder.dataplane.flush_policy.max_syscalls, 64);
        assert!(builder.dataplane.validate().is_ok());
    }
}
