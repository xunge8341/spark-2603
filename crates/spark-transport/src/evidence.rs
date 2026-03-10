use std::sync::Arc;

use crate::uci::EvidenceEvent;

/// Evidence event sink (OPS/Contract Tests shared semantics).
///
/// 设计约束：
/// - `EvidenceSink` 是典型的 **边界 trait object**：只允许在装配/边界处使用 `dyn`；
/// - 热路径只负责 `emit(event)`，不允许 `Any downcast`、不允许反射式分派。
pub trait EvidenceSink: Send + Sync {
    fn emit(&self, event: EvidenceEvent);
}

/// Evidence handle used by dataplane hot paths.
///
/// Rationale:
/// - Each channel holds a copy of the evidence handle, so it must be cheap to clone.
/// - Leaf backends can use `Arc<ConcreteSink>` to keep static dispatch without forcing `dyn`.
pub trait EvidenceHandle: EvidenceSink + Clone {}

impl<T> EvidenceHandle for T where T: EvidenceSink + Clone {}


/// Blanket impl: allow using `Arc<T>` as an evidence sink without forcing `dyn`.
///
/// Motivation:
/// - Leaf crates may want `Arc<ConcreteSink>` (static dispatch) while the contract suite
///   may use `Arc<dyn EvidenceSink>` (dynamic dispatch).
impl<T> EvidenceSink for Arc<T>
where
    T: EvidenceSink + ?Sized,
{
    #[inline]
    fn emit(&self, event: EvidenceEvent) {
        (**self).emit(event)
    }
}

/// Default no-op sink.
#[derive(Debug, Default)]
pub struct NoopEvidenceSink;

impl EvidenceSink for NoopEvidenceSink {
    fn emit(&self, _event: EvidenceEvent) {}
}

/// Composite sink.
///
/// 说明：
/// - `EvidenceSink` 通常不会实现 `Debug`；因此这里实现“结构性 Debug”，只打印 sink 数量。
#[derive(Clone)]
pub struct CompositeEvidenceSink {
    sinks: Box<[Arc<dyn EvidenceSink>]>,
}

impl core::fmt::Debug for CompositeEvidenceSink {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CompositeEvidenceSink")
            .field("sinks_len", &self.sinks.len())
            .finish()
    }
}

impl CompositeEvidenceSink {
    pub fn new(sinks: Vec<Arc<dyn EvidenceSink>>) -> Self {
        Self {
            sinks: sinks.into_boxed_slice(),
        }
    }
}

impl EvidenceSink for CompositeEvidenceSink {
    fn emit(&self, event: EvidenceEvent) {
        for s in self.sinks.iter() {
            s.emit(event);
        }
    }
}
