//! IO 抽象层（Driver-Abstraction Pattern 的核心之一）。
//!
//! 给不熟 Rust 的同学的提示：
//! - 在 OOP/GC 语言里，你可能会把 IO 统一抽象成一个基类/接口，然后到处传“对象引用”。
//! - 在 Rust 里，我们更推荐把“能力”表达为 **trait**（这里的 [`IoOps`]），并让后端实现它。
//! - **热路径**建议使用泛型（静态分发，零开销）；只有在边界处才用 `dyn Trait`。

pub mod caps;
mod accept;
mod connect;
mod error;
mod io_ops;
mod msg_boundary;
mod read_data;
mod read_outcome;
mod udp;
mod util;

pub use caps::*;
// 对外只暴露 IoOps。
//
// 说明：早期版本里曾把 IoOps 叫做 AbstractChannel（更偏 OOP 的命名）。
// Rust 社区更推荐直接用“能力 trait”的真实名称（IoOps），避免出现同义别名导致
// 新同学困惑、以及 deprecated re-export 带来的 warning 噪音。
pub use io_ops::IoOps;
pub use msg_boundary::MsgBoundary;
pub use read_data::ReadData;
pub use read_outcome::ReadOutcome;
pub use util::unsupported;
pub use accept::{classify_accept_error, AcceptDecision};
pub use connect::{classify_connect_error, ConnectDecision};
pub use error::classify_io_error;
pub use udp::classify_connected_udp_recv_error;

pub use spark_uci::{KernelError, Result, RxToken};
