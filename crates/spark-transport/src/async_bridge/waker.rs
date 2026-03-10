use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::task::{Wake, Waker};

use crate::TaskToken;

#[derive(Debug, Default)]
pub(super) struct WakeQueue {
    q: Mutex<VecDeque<TaskToken>>,
}

impl WakeQueue {
    pub(super) fn push(&self, tok: TaskToken) {
        self.q.lock().unwrap_or_else(|e| e.into_inner()).push_back(tok);
    }

    pub(super) fn pop(&self) -> Option<TaskToken> {
        self.q.lock().unwrap_or_else(|e| e.into_inner()).pop_front()
    }
}

pub(super) fn token_waker(wake_q: Arc<WakeQueue>, token: TaskToken) -> Waker {
    struct TokenWake {
        wake_q: Arc<WakeQueue>,
        token: TaskToken,
    }

    impl Wake for TokenWake {
        fn wake(self: Arc<Self>) {
            self.wake_q.push(self.token);
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.wake_q.push(self.token);
        }
    }

    Waker::from(Arc::new(TokenWake { wake_q, token }))
}
