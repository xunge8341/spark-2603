//! Leaf-level evidence adaptors for the mio backend.
//!
//! 目标：把 logging/metrics 生态适配全部下沉到叶子层，避免在 `spark-transport` 主干中引入
//! 第三方依赖/feature 噪音（Cargo feature unification）。

use std::sync::Arc;

use spark_transport::evidence::EvidenceSink;
use spark_transport::uci::EvidenceEvent;
use spark_transport::uci::names::evidence as ev_names;
use spark_transport::DataPlaneMetrics;

/// Mio backend evidence sink (static dispatch).
///
/// 设计点：
/// - 把 counters 映射与可选 log 输出合并成一个 sink，避免组合器/动态分派；
/// - 每个 channel 只 clone 这个小对象（内部持有 `Arc<DataPlaneMetrics>`），开销可忽略。
#[derive(Debug, Clone)]
pub struct MioEvidence {
    metrics: Arc<DataPlaneMetrics>,
    enable_log: bool,
}

impl MioEvidence {
    #[inline]
    pub fn new(metrics: Arc<DataPlaneMetrics>, enable_log: bool) -> Self {
        Self { metrics, enable_log }
    }
}

impl EvidenceSink for MioEvidence {
    fn emit(&self, event: EvidenceEvent) {
        use std::sync::atomic::Ordering;

        match event.name {
            ev_names::BACKPRESSURE_ENTER => {
                self.metrics
                    .backpressure_enter_total
                    .fetch_add(1, Ordering::Relaxed);
            }
            ev_names::BACKPRESSURE_EXIT => {
                self.metrics
                    .backpressure_exit_total
                    .fetch_add(1, Ordering::Relaxed);
            }
            ev_names::DRAINING_ENTER => {
                self.metrics
                    .draining_enter_total
                    .fetch_add(1, Ordering::Relaxed);
            }
            ev_names::DRAINING_EXIT => {
                self.metrics
                    .draining_exit_total
                    .fetch_add(1, Ordering::Relaxed);
            }
            ev_names::DRAINING_TIMEOUT => {
                self.metrics
                    .draining_timeout_total
                    .fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }

        if self.enable_log {
            #[cfg(feature = "evidence-log")]
            {
                log::info!(
                    "evidence name={} reason={} value={} unit={} unit_mapping={} scope={} scope_id={} chan_id={} pending_write_bytes={} inflight={}",
                    event.name,
                    event.reason,
                    event.value,
                    event.unit,
                    event.unit_mapping,
                    event.scope,
                    event.scope_id,
                    event.channel_id,
                    event.pending_write_bytes,
                    event.inflight
                );
            }

            #[cfg(not(feature = "evidence-log"))]
            {
                // runtime asked for log, but feature is off: keep leaf crate dependency-free.
                let _ = event;
            }
        }
    }
}

/// Preferred builder: returns a concrete sink (static dispatch).
#[inline]
pub fn build_default_evidence(metrics: Arc<DataPlaneMetrics>, enable_log: bool) -> MioEvidence {
    MioEvidence::new(metrics, enable_log)
}

/// Back-compat builder: returns a trait object (dynamic dispatch).
///
/// Use this only if you must pass evidence across an ABI boundary.
#[inline]
pub fn build_default_evidence_sink(metrics: Arc<DataPlaneMetrics>, enable_log: bool) -> Arc<dyn EvidenceSink> {
    Arc::new(MioEvidence::new(metrics, enable_log))
}
