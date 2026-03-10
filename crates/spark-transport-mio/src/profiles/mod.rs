//! Profile entry points for the mio backend.
//!
//! Goal: make the "default user path" discoverable without requiring users to understand generic parameters.
//! Advanced users can still use `MioBridge<A>` / `ChannelDriver<...>` directly.

pub mod tcp;
pub mod udp_connected;
