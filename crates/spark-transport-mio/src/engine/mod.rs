//! mio 后端的“驱动实现层”（Driver/Engine）。
//!
//! Rust 社区常用模式：Driver-Abstraction Pattern。
//! - `spark-transport` 只定义抽象与语义契约（traits + 状态机）；
//! - 具体后端（mio/uring/serial/…）在叶子层实现这些抽象；
//! - 热路径优先静态分发（泛型/单态化），避免 OOP 式虚调用层层叠加。
//!
//! 本模块提供：
//! - `MioReactor`: 基于 `mio::Poll` 的事件源；
//! - `QueueExecutor`: 简单 FIFO 任务队列执行器。

mod mio_reactor;
mod queue_executor;

pub use mio_reactor::{MioReactor, TcpStreamLookup, UdpSocketLookup};
pub use queue_executor::QueueExecutor;
