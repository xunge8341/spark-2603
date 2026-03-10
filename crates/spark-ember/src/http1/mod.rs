//! HTTP/1.1 management-plane adapter.
//!
//! The Spark management router is protocol-agnostic. This module provides a tiny
//! std-only HTTP/1.1 adapter that maps `(method, path)` to `(kind, path)`.

mod server;
mod state;

#[cfg(feature = "transport-mgmt")]
mod transport_server;

pub use server::Server;
pub use state::EmberState;

#[cfg(feature = "transport-mgmt")]
pub use transport_server::{HttpMgmtService, SpawnedTcpDataplane, TransportServer, TransportServerHandle};
