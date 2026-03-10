//! Route-level metrics for the management plane.
//!
//! Goals:
//! - Low overhead (mgmt QPS is typically low, but metrics should not be noisy).
//! - Low cardinality: label by stable `route_id` (user-provided when mapping routes).
//! - No external deps: atomics + std collections.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

#[derive(Debug, Default)]
pub struct RouteMetrics {
    routes: RwLock<BTreeMap<Box<str>, Arc<RouteCounter>>>,
}

impl RouteMetrics {
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register (or fetch) a route counter by stable route id.
    pub fn register(&self, route_id: Box<str>) -> Arc<RouteCounter> {
        // Fast path: read lock.
        {
            let g = match self.routes.read() {
                Ok(g) => g,
                Err(poisoned) => poisoned.into_inner(),
            };
            if let Some(c) = g.get(route_id.as_ref()) {
                return Arc::clone(c);
            }
        }

        let mut g = match self.routes.write() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(c) = g.get(route_id.as_ref()) {
            return Arc::clone(c);
        }
        let c = Arc::new(RouteCounter::default());
        g.insert(route_id, Arc::clone(&c));
        c
    }

    pub fn snapshot(&self) -> Vec<RouteCounterSnapshot> {
        let g = match self.routes.read() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        g.iter()
            .map(|(id, c)| RouteCounterSnapshot {
                route_id: id.to_string().into_boxed_str(),
                requests_total: c.requests_total.load(Ordering::Relaxed),
                responses_2xx_total: c.responses_2xx_total.load(Ordering::Relaxed),
                responses_4xx_total: c.responses_4xx_total.load(Ordering::Relaxed),
                responses_5xx_total: c.responses_5xx_total.load(Ordering::Relaxed),
                errors_total: c.errors_total.load(Ordering::Relaxed),
                inflight: c.inflight.load(Ordering::Relaxed),
                duration_nanos_total: c.duration_nanos_total.load(Ordering::Relaxed),
                duration_count: c.duration_count.load(Ordering::Relaxed),
            })
            .collect()
    }

    /// Render management route metrics to Prometheus text format.
    pub fn render_prometheus(&self) -> String {
        let snaps = self.snapshot();

        let mut out = String::new();
        out.push_str("# TYPE spark_mgmt_route_requests_total counter\n");
        out.push_str("# TYPE spark_mgmt_route_responses_2xx_total counter\n");
        out.push_str("# TYPE spark_mgmt_route_responses_4xx_total counter\n");
        out.push_str("# TYPE spark_mgmt_route_responses_5xx_total counter\n");
        out.push_str("# TYPE spark_mgmt_route_errors_total counter\n");
        out.push_str("# TYPE spark_mgmt_route_inflight gauge\n");
        out.push_str("# TYPE spark_mgmt_route_duration_nanos_total counter\n");
        out.push_str("# TYPE spark_mgmt_route_duration_count counter\n");

        for s in snaps {
            let id = escape_label_value(&s.route_id);
            out.push_str(&format!("spark_mgmt_route_requests_total{{route=\"{}\"}} {}\n", id, s.requests_total));
            out.push_str(&format!("spark_mgmt_route_responses_2xx_total{{route=\"{}\"}} {}\n", id, s.responses_2xx_total));
            out.push_str(&format!("spark_mgmt_route_responses_4xx_total{{route=\"{}\"}} {}\n", id, s.responses_4xx_total));
            out.push_str(&format!("spark_mgmt_route_responses_5xx_total{{route=\"{}\"}} {}\n", id, s.responses_5xx_total));
            out.push_str(&format!("spark_mgmt_route_errors_total{{route=\"{}\"}} {}\n", id, s.errors_total));
            out.push_str(&format!("spark_mgmt_route_inflight{{route=\"{}\"}} {}\n", id, s.inflight));
            out.push_str(&format!("spark_mgmt_route_duration_nanos_total{{route=\"{}\"}} {}\n", id, s.duration_nanos_total));
            out.push_str(&format!("spark_mgmt_route_duration_count{{route=\"{}\"}} {}\n", id, s.duration_count));
        }

        out
    }
}

#[derive(Debug, Default)]
pub struct RouteCounter {
    requests_total: AtomicU64,
    responses_2xx_total: AtomicU64,
    responses_4xx_total: AtomicU64,
    responses_5xx_total: AtomicU64,
    errors_total: AtomicU64,
    inflight: AtomicI64,
    duration_nanos_total: AtomicU64,
    duration_count: AtomicU64,
}

impl RouteCounter {
    #[inline]
    pub fn begin(self: &Arc<Self>) -> RouteGuard {
        self.requests_total.fetch_add(1, Ordering::Relaxed);
        self.inflight.fetch_add(1, Ordering::Relaxed);
        RouteGuard { counter: Arc::clone(self) }
    }

    fn end(&self, status: u16, elapsed: Duration) {
        let class = status / 100;
        match class {
            2 => {
                self.responses_2xx_total.fetch_add(1, Ordering::Relaxed);
            }
            4 => {
                self.responses_4xx_total.fetch_add(1, Ordering::Relaxed);
            }
            5 => {
                self.responses_5xx_total.fetch_add(1, Ordering::Relaxed);
                self.errors_total.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }

        self.duration_nanos_total
            .fetch_add(elapsed.as_nanos().min(u128::from(u64::MAX)) as u64, Ordering::Relaxed);
        self.duration_count.fetch_add(1, Ordering::Relaxed);
        self.inflight.fetch_sub(1, Ordering::Relaxed);
    }
}

pub struct RouteGuard {
    counter: Arc<RouteCounter>,
}

impl RouteGuard {
    #[inline]
    pub fn finish(self, status: u16, elapsed: Duration) {
        self.counter.end(status, elapsed);
    }
}

#[derive(Debug, Clone)]
pub struct RouteCounterSnapshot {
    pub route_id: Box<str>,
    pub requests_total: u64,
    pub responses_2xx_total: u64,
    pub responses_4xx_total: u64,
    pub responses_5xx_total: u64,
    pub errors_total: u64,
    pub inflight: i64,
    pub duration_nanos_total: u64,
    pub duration_count: u64,
}

fn escape_label_value(s: &str) -> String {
    // Minimal Prometheus label escaping.
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::RouteMetrics;

    #[test]
    fn render_includes_route_id_labels() {
        let rm = RouteMetrics::new();
        let c = rm.register("metrics".to_string().into_boxed_str());
        let g = c.begin();
        g.finish(200, std::time::Duration::from_millis(1));

        let text = rm.render_prometheus();
        assert!(text.contains("spark_mgmt_route_requests_total"));
        assert!(text.contains("route=\"metrics\""));
    }
}