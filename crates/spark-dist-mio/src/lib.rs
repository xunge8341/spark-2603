//! `spark-dist-mio`: default distribution (delivery crate) for the mio backend.
//!
//! Design goals (aligned with the handoff principles):
//! - Keep the core (`spark-transport`) semantics/data-structures clean and runtime-neutral.
//! - Dist crate only does **assembly and default UX**, similar to ASP.NET Core's default hosting experience.
//! - Make the internal ecosystem self-bootstrapping: a single crate can run a working server.

mod tcp;
mod udp_connected;
mod server;

pub mod prelude;
pub mod profiles;

pub use tcp::run_tcp_default;
pub use udp_connected::run_udp_connected_default;

pub use server::{Server, ServerBuilder, Transport};
