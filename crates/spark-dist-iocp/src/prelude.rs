//! Convenience re-exports for bootstrapping internal integrations.
//!
//! This mirrors `spark-dist-mio` to keep the developer experience consistent.

pub use spark_host::{HostBuilder, HostSpec, NoPipeline, ServerConfig};
pub use spark_transport::{DataPlaneConfig, DataPlaneMetrics, DataPlaneOptions};

pub use crate::server::{Server, ServerBuilder, Transport};
