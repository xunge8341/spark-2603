use spark_host::router::MgmtState;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Debug)]
pub struct EmberState {
    draining: Arc<AtomicBool>,
    listener_ready: Arc<AtomicBool>,
    dependencies_ready: Arc<AtomicBool>,
    accepting_new_requests: Arc<AtomicBool>,
    active_requests: Arc<AtomicUsize>,
    overloaded: Arc<AtomicBool>,
}

impl EmberState {
    #[inline]
    pub fn new() -> Self {
        Self {
            draining: Arc::new(AtomicBool::new(false)),
            listener_ready: Arc::new(AtomicBool::new(false)),
            dependencies_ready: Arc::new(AtomicBool::new(true)),
            accepting_new_requests: Arc::new(AtomicBool::new(true)),
            active_requests: Arc::new(AtomicUsize::new(0)),
            overloaded: Arc::new(AtomicBool::new(false)),
        }
    }

    #[inline]
    pub fn set_draining(&self, v: bool) {
        self.draining.store(v, Ordering::Release);
        if v {
            self.accepting_new_requests.store(false, Ordering::Release);
        }
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
    pub fn set_accepting_new_requests(&self, v: bool) {
        self.accepting_new_requests.store(v, Ordering::Release);
    }

    #[inline]
    pub fn inc_active_requests(&self) {
        self.active_requests.fetch_add(1, Ordering::AcqRel);
    }

    #[inline]
    pub fn dec_active_requests(&self) {
        let _ = self
            .active_requests
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |cur| {
                if cur == 0 {
                    None
                } else {
                    Some(cur - 1)
                }
            });
    }

    #[inline]
    pub fn active_requests(&self) -> usize {
        self.active_requests.load(Ordering::Acquire)
    }

    #[inline]
    pub fn set_overloaded(&self, v: bool) {
        self.overloaded.store(v, Ordering::Release);
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
    fn is_accepting_new_requests(&self) -> bool {
        self.accepting_new_requests.load(Ordering::Acquire)
            && !self.draining.load(Ordering::Acquire)
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

    #[inline]
    fn is_overloaded(&self) -> bool {
        self.overloaded.load(Ordering::Acquire)
    }
}
