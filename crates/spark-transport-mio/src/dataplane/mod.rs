//! Dataplane entry points (leaf-only, mio backend).
//!
//! These functions intentionally construct the driver inside the dataplane thread.
//! This keeps Netty-style single-thread event-loop semantics (`!Send` handlers/futures)
//! without requiring the whole driver to be `Send`.

pub mod tcp;
pub mod udp_connected;
