//! `spark-dist-iocp`: Windows-first distribution (delivery crate) for the IOCP path.
//!
//! DECISION (BigStep-11, dogfooding + multi-backend):
//! - Keep `spark-transport` runtime-neutral and clean.
//! - Provide a Windows distribution crate that wires the host+mgmt-plane onto the IOCP backend boundary.
//! - This crate is the **self-bootstrapping** entry point for internal products on Windows.
//!
//! Notes:
//! - The current IOCP backend is in *phase-0 bring-up* (see `spark-transport-iocp`).
//! - The goal is to make the distribution surface stable while the backend implementation evolves.

mod tcp;
mod server;

pub mod prelude;

pub use tcp::run_tcp_default;

pub use server::{Server, ServerBuilder, Transport};
