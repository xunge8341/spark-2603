//! Prometheus text-format exporter for dataplane metrics.
//!
//! Design:
//! - `spark-transport` owns the runtime-neutral counter struct (`DataPlaneMetrics`).
//! - exporters live in leaf crates to keep the core minimal and dependency-clean.

mod render;

pub use render::render_prometheus;
