//! `spark-transport-mio`: mio backend integration (dataplane).
//!
//! This crate is a typical **Driver-Abstraction Pattern** (common in the Rust ecosystem):
//! - `spark-transport` defines runtime-neutral semantics and data structures (Channel/Pipeline/backpressure ordering).
//! - This crate is the leaf implementation that maps those semantics onto a concrete I/O backend (mio).
//!
//! Design goals:
//! - **Dependency hygiene**: mio stays in the leaf layer; core crates don't depend on it.
//! - **Replaceable backend**: future backends can be added (`spark-transport-uring`, `spark-transport-serial`, ...).
//! - **Hot-path clarity**: minimize dynamic dispatch in the dataplane; use explicit enums where possible.

pub mod engine;
pub mod evidence;
pub mod io;
pub mod tcp;
pub mod udp;
pub mod profiles;

mod acceptor;
mod dataplane;
mod pending;

pub use dataplane::tcp::{spawn_tcp_dataplane, try_spawn_tcp_dataplane, try_spawn_tcp_dataplane_with_framing, TcpDataplaneHandle};
pub use dataplane::udp_connected::{spawn_udp_dataplane, try_spawn_udp_dataplane};
pub use pending::{apply_pending_tcp, apply_pending_udp};

pub use engine::{MioReactor, QueueExecutor, TcpStreamLookup, UdpSocketLookup};
pub use io::MioIo;
pub use tcp::TcpChannel;
pub use udp::{MemDatagramChannel, UdpSocketChannel};

use spark_transport::AsyncBridge;

/// Convenience alias: mio backend dataplane driver with leaf evidence type.
///
/// - Developer-friendly: most users don't need to care about the evidence generic.
/// - Hot-path: mio backend defaults to `MioEvidence` (static dispatch; no vtable in the dataplane).
pub type MioBridge<A> =
    AsyncBridge<MioReactor, QueueExecutor, A, crate::evidence::MioEvidence, MioIo>;

/// Default TCP profile alias.
pub type MioBridgeTcp<A> = MioBridge<A>;

/// Default connected-UDP profile alias.
pub type MioBridgeUdp<A> = MioBridge<A>;
