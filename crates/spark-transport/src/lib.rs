//! Transport 语义层（对齐 Netty 的 Channel/Pipeline/Backpressure）。
//!
//! 设计定位：
//! - **语义中枢**：把“网络/串口等 IO 后端”统一抽象成 `IoOps`，并在此之上实现 Netty 风格的
//!   Channel 状态机（cumulation / decoder loop / outbound buffer / watermarks）。
//! - **运行时中立**：不直接依赖 mio/tokio/io_uring 等具体实现；这些属于后端集成 crate。
//! - **契约可测**：配合 `spark-transport-contract` 做一致性验证。
//!
//! ## 热路径原则（必须遵守）
//! 
//! 目标：防止“OOP 回潮”（反射/容器/运行时多态贯穿主干），保持可预测性能。
//!
//! - **禁止热路径 downcast**：`Any::downcast_*` 只能出现在测试/诊断或叶子层后端边界（例如
//!   interest 变更落地时的 *lookup*），不允许出现在 read/flush/tick 循环。
//! - **dyn 仅边界**：`dyn Trait` 只用于系统边界（如 `DynChannel`/`EvidenceSink`），主干逻辑用泛型。
//! - **泛型单态化为主**：driver/engine/app 等主干组件优先 static dispatch（像 embedded-hal/sqlx）。
//!
//! ## 开发者友好约定
//!
//! - 默认（bring-up）配置使用 `dyn` 作为**边界**：
//!   - `async_bridge::dyn_boundary` 提供默认别名（`Arc<dyn EvidenceSink>` + `Box<dyn DynChannel>`）。
//! - 叶子层可按需把 hot path 静态化：
//!   - mio：`MioIo`（enum 分发）+ `MioEvidence`（leaf sink）。
//! - 运行时行为通过小型策略对象表达（见 `policy` 模块），默认策略覆盖常见场景。

pub use spark_uci as uci;
pub use spark_uci::{Budget, KernelError, Result, RxToken, TaskToken};

mod app;

pub mod io;
pub mod reactor;
pub mod executor;
pub mod policy;

pub mod async_bridge;
pub mod evidence;
pub mod lease;
pub mod error_codes;
pub mod metrics;
pub mod slab;

pub mod config;

pub use app::App;
pub use async_bridge::AsyncBridge;

pub use evidence::{CompositeEvidenceSink, EvidenceSink, NoopEvidenceSink};

// Re-export commonly used types.
pub use config::{
    DataPlaneConfig, DataPlaneConfigError, DataPlaneDiagnostics, DataPlaneLimits, DataPlaneOptions,
};
pub use metrics::{DataPlaneDerivedMetrics, DataPlaneMetrics, DataPlaneMetricsSnapshot};
