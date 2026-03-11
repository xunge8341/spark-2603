//! Netty/DotNetty 风格的 ChannelPipeline（bring-up 版本）。
//!
//! 设计目标：
//! - 清晰表达 inbound/outbound 双向事件语义。
//! - handler 默认“透传”（调用 ctx.fire_* / ctx.write / ctx.flush）。
//! - 异步业务（Service/Future）不进入 handler 签名：由 `AppServiceHandler` 内部创建 future，
//!   driver 负责 poll；这与 DotNetty 的工程经验一致（pipeline 同步、业务异步通过 Task 承载）。

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

// 这些 re-export 是 pipeline 的对外 API（供上层组装/扩展）。
// 当前 crate 内部未必直接引用，避免触发 unused_imports 的噪声。
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
