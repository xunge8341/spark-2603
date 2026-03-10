use spark_host::router::MgmtState;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Minimal server state shared with management-plane handlers.
///
/// This is intentionally tiny and runtime-neutral: no async runtime, no reactor.
#[derive(Debug)]
pub struct EmberState {
    draining: Arc<AtomicBool>,
}

impl EmberState {
    #[inline]
    pub fn new() -> Self {
        Self { draining: Arc::new(AtomicBool::new(false)) }
    }

    #[inline]
    pub fn set_draining(&self, v: bool) {
        self.draining.store(v, Ordering::Release);
    }

    #[inline]
    pub fn draining_handle(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.draining)
    }
}

impl Default for EmberState {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl MgmtState for EmberState {
    #[inline]
    fn is_draining(&self) -> bool {
        self.draining.load(Ordering::Acquire)
    }

    #[inline]
    fn request_draining(&self) {
        self.set_draining(true);
    }
}
