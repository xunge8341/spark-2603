//! Minimal example: group-level request timeout for mgmt routes.
//!
//! Run:
//!   cargo run -p spark-dist-mio --example mgmt_timeout_group

use spark_buffer::Bytes;
use spark_core::context::Context;
use spark_core::service::Service;
use spark_dist_mio::ServerBuilder;
use spark_host::router::MgmtResponse;
use spark_transport::KernelError;

use std::time::Duration;

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
        .map_group("/admin", |g| {
            g.with_request_timeout(Duration::from_secs(2));
            g.map_get("/ping", |_req| async { MgmtResponse::ok("pong") })
                .named("admin_ping");
            g.map_post("/reload", |_req| async { MgmtResponse::ok("reload accepted") })
                .named("admin_reload");
        })
        .pipeline(|pb| pb.service(Noop))
        .build()
        .map_err(|_e| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "HostBuilder build failed")
        })?;

    server.run()
}
