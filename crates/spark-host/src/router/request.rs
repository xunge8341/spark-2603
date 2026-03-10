use super::types::RouteKind;
use std::sync::Arc;

/// Minimal state interface visible to management handlers.
pub trait MgmtState: Send + Sync {
    fn is_draining(&self) -> bool;

    /// Request entering draining state (default no-op).
    fn request_draining(&self) {
        // Optional capability.
    }
}

#[derive(Clone)]
pub struct MgmtRequest {
    pub kind: RouteKind,
    pub path: Box<str>,
    pub body: Vec<u8>,
    pub state: Arc<dyn MgmtState>,
}

impl MgmtRequest {
    #[inline]
    pub fn path_str(&self) -> &str {
        &self.path
    }

    #[inline]
    pub fn kind_str(&self) -> &str {
        self.kind.as_str()
    }
}

// Keep Debug minimal: do not require MgmtState: Debug.
impl std::fmt::Debug for MgmtRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MgmtRequest")
            .field("kind", &self.kind)
            .field("path", &self.path)
            .field("body_len", &self.body.len())
            .field("state", &"<MgmtState>")
            .finish()
    }
}
