//! Convenience re-exports for bootstrapping internal integrations.
//!
//! This is intentionally small and stable: it helps the internal ecosystem (examples, smoke tests,
//! higher-level products) self-bootstrap without pulling in backend details.

pub use spark_host::{HostBuilder, HostSpec, NoPipeline, ServerConfig};
pub use spark_transport::{DataPlaneConfig, DataPlaneMetrics, DataPlaneOptions};

pub use crate::server::{Server, ServerBuilder, Transport};
