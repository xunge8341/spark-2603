//! Host 装配层（对齐 ASP.NET Core 的 Hosting/DI/Endpoint Routing 心智）。
//!
//! 设计定位：
//! - 只负责“装配与配置”：`HostBuilder -> HostSpec`。
//! - **不绑定运行时**：Host 本身不负责 listen/accept/驱动事件循环。
//! - 运行（run）由上层应用或 server 发行物（如 `spark-ember`）完成。

pub mod builder;
pub mod config;
pub mod mgmt_profile;
pub mod router;
pub mod route_metrics;

pub use builder::{HostBuilder, HostSpec, NoPipeline};
pub use config::ServerConfig;
pub use mgmt_profile::{MgmtHttpLimits, MgmtIsolationOptions, MgmtTransportProfileV1};

pub use route_metrics::RouteMetrics;

