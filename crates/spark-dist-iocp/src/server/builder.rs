use super::instance::{Server, Transport};
use spark_buffer::Bytes;
use spark_core::service::Service;
use spark_host::builder::{HostBuilder, HostSpec, NoPipeline};
use spark_host::config::ServerConfig;
use spark_transport::{DataPlaneConfig, DataPlaneOptions, KernelError};

/// Distribution-layer builder (Netty/Kestrel inspired, Rust-idiomatic).
///
/// This wraps `spark-host::HostBuilder` and adds a transport selector.
pub struct ServerBuilder<S> {
    host: HostBuilder<S>,
    transport: Transport,
}

impl ServerBuilder<NoPipeline> {
    /// Create a builder with sane defaults.
    ///
    /// Includes default mgmt endpoints (/healthz, /readyz, /metrics, /drain).
    pub fn new() -> Self {
        Self {
            host: HostBuilder::new().use_default_diagnostics(),
            transport: Transport::Tcp,
        }
    }

    pub fn tcp(mut self) -> Self {
        self.transport = Transport::Tcp;
        self
    }

    pub fn config(mut self, cfg: ServerConfig) -> Self {
        self.host = self.host.config(cfg);
        self
    }

    pub fn dataplane(mut self, cfg: DataPlaneConfig) -> Self {
        self.host = self.host.dataplane(cfg);
        self
    }

    pub fn dataplane_options(mut self, options: DataPlaneOptions) -> Self {
        self.host = self.host.dataplane_options(options);
        self
    }

    pub fn pipeline<F, Svc>(self, build: F) -> ServerBuilder<Svc>
    where
        F: FnOnce(spark_core::pipeline::PipelineBuilder<spark_core::pipeline::IdentityLayer>) -> Svc,
        Svc: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
    {
        ServerBuilder {
            host: self.host.pipeline(build),
            transport: self.transport,
        }
    }
}

impl<S> ServerBuilder<S> {
    /// Apply the canonical transport-backed management profile.
    pub fn use_transport_mgmt_profile(mut self) -> Self {
        self.host = self.host.use_transport_mgmt_profile();
        self
    }

    /// Apply a throughput-oriented tuning overlay to the current dataplane shape.
    pub fn use_transport_perf_profile(mut self) -> Self {
        self.host = self.host.use_transport_perf_profile();
        self
    }

    /// Apply the canonical transport-backed management profile with throughput-oriented tuning.
    pub fn use_transport_mgmt_perf_profile(mut self) -> Self {
        self.host = self.host.use_transport_mgmt_perf_profile();
        self
    }

    pub fn map_get<F, Fut>(
        mut self,
        path: impl Into<Box<str>>,
        route_id: impl Into<Box<str>>,
        handler: F,
    ) -> Self
    where
        F: Fn(spark_host::router::MgmtRequest) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = spark_host::router::MgmtResponse> + Send + 'static,
    {
        self.host = self.host.map_get(path, route_id, handler);
        self
    }

    pub fn map_post<F, Fut>(
        mut self,
        path: impl Into<Box<str>>,
        route_id: impl Into<Box<str>>,
        handler: F,
    ) -> Self
    where
        F: Fn(spark_host::router::MgmtRequest) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = spark_host::router::MgmtResponse> + Send + 'static,
    {
        self.host = self.host.map_post(path, route_id, handler);
        self
    }

    pub fn map_group(mut self, prefix: &'static str, f: impl FnOnce(&mut spark_host::router::MgmtGroup<'_>)) -> Self {
        self.host = self.host.map_group(prefix, f);
        self
    }
}

impl<S> ServerBuilder<S>
where
    S: Service<Bytes, Response = Option<Bytes>, Error = KernelError> + Send + Sync + 'static,
{
    pub fn build(self) -> core::result::Result<Server<S>, KernelError> {
        let spec: HostSpec<S> = self.host.build()?;
        Ok(Server {
            spec,
            transport: self.transport,
        })
    }
}

impl Default for ServerBuilder<NoPipeline> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}
