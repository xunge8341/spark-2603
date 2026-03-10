use spark_uci::Instant;

/// Business context. Moved into async state machines to avoid self-referential futures.
#[derive(Debug, Clone, Default)]
pub struct Context {
    pub deadline: Option<Instant>,
    pub cancelled: bool,
    pub trace_id: Option<[u8; 16]>,
    pub route_id: Option<&'static str>,
}

