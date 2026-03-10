//! Minimal "internal ecosystem bootstrap" example.
//!
//! Run:
//!   cargo run -p spark-dist-mio --example tcp_default

use spark_buffer::Bytes;
use spark_core::{Context, Service};
use spark_dist_mio::prelude::HostBuilder;
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
    let spec = HostBuilder::new()
        .pipeline(|_p| Noop)
        .use_default_diagnostics()
        .build().map_err(|_e| std::io::Error::new(std::io::ErrorKind::InvalidInput, "HostBuilder build failed"))?;

    spark_dist_mio::run_tcp_default(spec)
}
