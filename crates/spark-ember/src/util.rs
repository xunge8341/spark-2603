//! 小型工具模块（不引入第三方依赖）。
//!
//! 说明：
//! - `spark-host` 的 mgmt handler 使用 async 风格（返回 Future）。
//! - `spark-ember` 为保持运行时中立，不依赖 Tokio/async-std。
//! - 因此这里提供一个**极简**的 `block_on`：
//!   - 仅适用于“不会真正等待 IO”的 Future（多数 mgmt handler 都是纯计算/纯内存）。
//!   - 如果 handler 内部需要异步 IO，应由上层应用选择合适的 runtime 并在集成层执行。

use core::future::Future;
use core::task::{Context, Poll, Waker};
use std::sync::Arc;
use std::task::Wake;
use std::time::Instant;

fn noop_waker() -> Waker {
    struct Noop;
    impl Wake for Noop {
        fn wake(self: Arc<Self>) {}
        fn wake_by_ref(self: &Arc<Self>) {}
    }
    Waker::from(Arc::new(Noop))
}

/// 以“轮询到完成”的方式同步执行一个 Future。
///
/// 注意：如果 Future 返回 `Pending`，本实现会自旋继续 poll。
/// 因此它只适合 **不会真正阻塞等待外部事件** 的 Future。
pub fn block_on<F: Future>(fut: F) -> F::Output {
    let waker = noop_waker();
    let mut cx = Context::from_waker(&waker);
    let mut fut = Box::pin(fut);
    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(v) => return v,
            Poll::Pending => {
                // 自旋：mgmt handler 理论上不应长期 Pending。
                // 若发生，说明调用方需要引入真正的 runtime 集成层。
            }
        }
    }
}

pub fn block_on_until<F: Future>(fut: F, deadline: Instant) -> Option<F::Output> {
    let waker = noop_waker();
    let mut cx = Context::from_waker(&waker);
    let mut fut = Box::pin(fut);
    loop {
        if Instant::now() >= deadline {
            return None;
        }
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(v) => return Some(v),
            Poll::Pending => {}
        }
    }
}
