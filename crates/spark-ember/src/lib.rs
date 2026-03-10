//! Ember：默认 server 发行物。
//!
//! 重要设计点：
//! - **不绑定任何运行时**：不引入 Tokio/mio 等第三方 runtime。
//! - 控制面（mgmt-plane）提供一个 std-only 的最小实现：`Server`。
//! - 数据面（dataplane）由上层应用注入（例如 mio/io_uring/tokio 后端）。

pub mod http1;
pub mod server;
pub mod run;
mod util;

pub use http1::{EmberState, Server};

#[cfg(feature = "transport-mgmt")]
pub use http1::{TransportServer, TransportServerHandle};
pub use run::run_blocking;
