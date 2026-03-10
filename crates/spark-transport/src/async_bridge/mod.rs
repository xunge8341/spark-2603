//! 异步数据面桥接（bring-up 版本）。
//!
//! 目标（对齐 Netty / DotNetty 的经典语义）：
//! - 业务团队只需要写 **handler / codec**，不碰 event-loop / I/O 细节。
//! - `write()` 仅入队，`flush()` 才真正尝试写出；可写性由水位线驱动。
//! - TCP 必须走 **cumulation + decoder loop**（半包/粘包不外泄）。
//!
//! 代码结构约束（便于评审与长期维护）：
//! - `lib.rs` 仅做导出；本模块内部也尽量做到“一个抽象一个文件”。
//! - `pipeline/` 目录提供 Netty 风格的 `ChannelPipeline`（inbound/outbound 双向）。

pub mod channel;
mod channel_driver;
mod channel_state;
pub mod dyn_channel;
mod framers;
mod inbound_state;
mod outbound_buffer;
mod outbound_frame;
mod pipeline;
mod task;
mod waker;

pub use channel_driver::{chan_index, AsyncBridge, ChannelDriver, DriverConfig, DriverLimits};
pub use channel_state::tok_chan_id;
pub use outbound_frame::{OutboundFrame, OUTBOUND_INLINE_MAX, OUTBOUND_SEG_MAX};

// Framing profiles for built-in pipelines.
pub use pipeline::FrameDecoderProfile;

// Re-export codec primitives commonly needed for framing configuration.
pub use spark_codec::ByteOrder;

// Intentionally no alias exports here; `chan_index` is the single canonical helper.

/// Dyn boundary re-export (dev-friendly).
///
/// 说明：
/// - 这不是鼓励在热路径滥用 `dyn`，而是把 `dyn` 的“边界位置”显式化；
/// - 叶子层后端（mio/uring/serial）若需要把具体 IO 放进 `Box<dyn DynChannel>`，可直接实现此 trait。
pub use dyn_channel::DynChannel;

/// Dev-friendly aliases for the default dyn-boundary configuration.
///
/// 默认（bring-up）配置：
/// - Evidence: `Arc<dyn EvidenceSink>`
/// - IO: `Box<dyn DynChannel>`
///
/// 叶子层可按需将 IO/Evidence 静态化（例如 mio 的 `MioIo` + `MioEvidence`）。
pub mod dyn_boundary {
    use std::sync::Arc;

    use crate::evidence::EvidenceSink;

    pub type DynEvidence = Arc<dyn EvidenceSink>;
    pub type DynIo = Box<dyn super::DynChannel>;

    pub type ChannelState = super::channel_state::ChannelState<DynEvidence, DynIo>;
    pub type Channel<A> = super::channel::Channel<A, DynEvidence, DynIo>;
    pub type ChannelDriver<R, E, A> =
        super::channel_driver::ChannelDriver<R, E, A, DynEvidence, DynIo>;
}

/// Contract-test exports.
///
/// 说明：
/// - 该模块用于 `spark-transport-contract` 这类独立测试 crate。
/// - 不建议业务代码依赖此模块；生产使用应通过 dataplane/driver。
#[cfg(feature = "contract")]
pub mod contract {
    use std::sync::Arc;

    use crate::evidence::EvidenceSink;

    pub use super::channel::Channel;
    pub type ChannelState = super::channel_state::ChannelState<Arc<dyn EvidenceSink>>;
    pub use super::dyn_channel::DynChannel;
    pub use super::outbound_buffer::{
        FlushStatus, OutboundAllocEvidence, OutboundBuffer, WritabilityChange,
    };
}
