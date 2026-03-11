use spark_host::router::MgmtState;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug)]
pub struct EmberState {
    draining: Arc<AtomicBool>,
    listener_ready: Arc<AtomicBool>,
    dependencies_ready: Arc<AtomicBool>,
}

impl EmberState {
    #[inline]
    pub fn new() -> Self {
        Self {
            draining: Arc::new(AtomicBool::new(false)),
            listener_ready: Arc::new(AtomicBool::new(false)),
            dependencies_ready: Arc::new(AtomicBool::new(true)),
        }
    }

    #[inline]
    pub fn set_draining(&self, v: bool) {
        self.draining.store(v, Ordering::Release);
    }

    #[inline]
    pub fn set_listener_ready(&self, v: bool) {
        self.listener_ready.store(v, Ordering::Release);
    }

    #[inline]
    pub fn set_dependencies_ready(&self, v: bool) {
        self.dependencies_ready.store(v, Ordering::Release);
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

    #[inline]
    fn is_listener_ready(&self) -> bool {
        self.listener_ready.load(Ordering::Acquire)
    }

    #[inline]
    fn dependencies_ready(&self) -> bool {
        self.dependencies_ready.load(Ordering::Acquire)
    }
}
