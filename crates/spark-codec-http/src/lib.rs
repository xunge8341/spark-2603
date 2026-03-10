//! HTTP codecs for Spark.
//!
//! Design goals:
//! - Keep the **router** protocol-agnostic; HTTP is just an adapter.
//! - Provide a tiny, dependency-light HTTP/1.1 codec for Spark's internal control-plane.
//! - Stay Rust-idiomatic: explicit types, no global state, minimal allocations.

pub mod http1;
