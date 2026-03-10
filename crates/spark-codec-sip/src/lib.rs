#![no_std]

//! SIP codecs for Spark.
//!
//! This crate is primarily used as a **dogfooding** target for Spark's text-protocol
//! infrastructure. SIP is a mature, real-world text-shaped protocol with strict
//! start-line grammar and header semantics.

extern crate alloc;

pub mod sip2;
