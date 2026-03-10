//! TCP 通道（mio 后端）。
//!
//! 说明：
//! - 这是低层 IO backend（接近 Netty 的 `Channel.Unsafe`）。
//! - 上层语义（cumulation/decoder/outbound/backpressure）在 `spark-transport`。

mod rx_token;
mod tcp_channel;

pub use tcp_channel::TcpChannel;
