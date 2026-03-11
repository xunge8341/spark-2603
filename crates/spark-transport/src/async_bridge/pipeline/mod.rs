//! Netty/DotNetty 风格的 ChannelPipeline（bring-up 版本）。
//!
//! ## 稳定扩展面（public）
//! - `ChannelPipelineBuilder`：组装 handler 链和 framing 选项。
//! - `ChannelHandler`：自定义 inbound/outbound 处理逻辑。
//! - `AppServiceOptions` / `OverloadAction`：每 handler/per protocol 并发与过载策略。
//! - `FrameDecoderProfile` / `DelimiterSpec`：输入 framing 选择。
//!
//! ## internal（不承诺稳定）
//! `head` / `tail` / `context` / `event` 等模块属于运行时实现细节，不应被上层直接依赖。
//! `ChannelPipeline` 类型可通过本模块 re-export 使用，但其内部实现模块路径不视为稳定 contract。

mod builder;
mod channel_pipeline;
mod context;
mod event;
mod frame_decoder;
mod frame_encoder;
mod handler;
mod head;
mod profile;
mod service_handler;
mod tail;

// Optional mgmt HTTP/1 framing is kept in a dedicated module to reduce feature-gated clutter.
#[cfg(feature = "mgmt-http1")]
mod http1_inbound;

// ---- internal type aliases (keep signatures readable; satisfy clippy::type_complexity) ----
pub(crate) type HandlerBox<A, Ev, Io> = Box<dyn handler::ChannelHandler<A, Ev, Io>>;
pub(crate) type HandlerVec<A, Ev, Io> = Vec<HandlerBox<A, Ev, Io>>;

// 这些 re-export 是 pipeline 的稳定扩展入口（供上层组装/扩展）。
// 当前 crate 内部未必直接引用，避免触发 unused_imports 噪声。
#[allow(unused_imports)]
pub use builder::ChannelPipelineBuilder;
#[allow(unused_imports)]
pub use builder::{DelimiterOptions, Http1Options, LineOptions};
#[allow(unused_imports)]
pub use builder::{LengthFieldOptions, Varint32Options};
#[allow(unused_imports)]
pub use channel_pipeline::ChannelPipeline;
#[allow(unused_imports)]
pub use handler::ChannelHandler;
#[allow(unused_imports)]
pub use service_handler::{AppOverloadStats, AppServiceOptions, OverloadAction};

#[allow(unused_imports)]
pub use profile::FrameDecoderProfile;
pub use profile::{DelimiterSpec, MAX_DELIMITER_LEN};
