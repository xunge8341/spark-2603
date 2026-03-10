//! Product-level server API (distribution layer).
//!
//! Goals:
//! - Provide a Netty/Kestrel-like "mature" UX surface without polluting core crates.
//! - Keep the Rust expression idiomatic: builder + strongly-typed config + explicit run.

mod builder;
mod instance;

pub use builder::ServerBuilder;
pub use instance::{Server, Transport};
