use spark_buffer::Bytes;
use spark_core::context::Context;
use spark_core::service::Service;
use spark_dist_mio::ServerBuilder;
use spark_transport::KernelError;

struct Noop;

impl Service<Bytes> for Noop {
    type Response = Option<Bytes>;
    type Error = KernelError;

    async fn call(&self, _context: Context, _request: Bytes) -> Result<Self::Response, Self::Error> {
        Ok(None)
    }
}

fn main() -> std::io::Result<()> {
    // A Kestrel-like hosting experience:
    // - Defaults include /healthz /readyz /metrics /drain.
    // - Transport selection is explicit.
    let server = ServerBuilder::new()
        .tcp()
        .map_group("/admin", |g| {
            g.map_get("/ping", |_req| async { spark_host::router::MgmtResponse::ok("pong") })
                .named("admin_ping");
        })
        .pipeline(|pb| pb.service(Noop))
        .build().map_err(|_e| std::io::Error::new(std::io::ErrorKind::InvalidInput, "HostBuilder build failed"))?;

    server.run()
}
