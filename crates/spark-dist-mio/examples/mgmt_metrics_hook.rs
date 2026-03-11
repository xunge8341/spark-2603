//! Minimal example: attach diagnostics/metrics-oriented mgmt hooks.
//!
//! Run:
//!   cargo run -p spark-dist-mio --example mgmt_metrics_hook

use spark_buffer::Bytes;
use spark_core::context::Context;
use spark_core::service::Service;
use spark_dist_mio::ServerBuilder;
use spark_host::router::MgmtResponse;
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
    let server = ServerBuilder::new()
        .tcp()
        .map_get("/diag/state", "diag_state", |req| async move {
            let body = format!(
                "draining={} accepting={} listener_ready={} dependencies_ready={} overloaded={}\n",
                req.state.is_draining(),
                req.state.is_accepting_new_requests(),
                req.state.is_listener_ready(),
                req.state.dependencies_ready(),
                req.state.is_overloaded()
            );
            MgmtResponse::ok(body)
        })
        .pipeline(|pb| pb.service(Noop))
        .build()
        .map_err(|_e| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "HostBuilder build failed")
        })?;

    server.run()
}
