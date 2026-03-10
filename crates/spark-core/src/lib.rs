#![no_std]
#![forbid(unsafe_code)]

pub use spark_uci::{Instant, KernelError};

pub mod context;
pub mod layer;
pub mod service;
pub mod backpressure;
pub mod buffer;
pub mod pipeline;
pub mod router;

// Public API re-exports (stable crate root surface).
pub use context::Context;
pub use service::Service;
