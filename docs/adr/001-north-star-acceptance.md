# ADR-001 North Star & Acceptance Criteria（世界一流标准）

## 决策

Spark 的终态目标以 **工程可验收语言**冻结为以下三重硬指标：

1) **C++ 性能**：热路径可控分配、零拷贝优先、批量 syscall、p99/p999 稳定。
2) **Rust 安全**：unsafe 收敛可审计、panic-free 默认语义、并发语义可证明/可测。
3) **ASP.NET Core 体验**：Options 化装配、可观测性一等公民、Dogfooding 闭环。

同时承诺：

- **跨软硬件平台**：Windows(IOCP)/Linux(epoll/io_uring)/macOS(kqueue) 后端可插拔，语义由 contract suite 约束。
- **不限协议**：框架提供通用 transport + framing 基座，协议栈按需叠加。
- **可扩展到 Embedded/WASM**：核心语义不绑定 OS；通过 backend 插槽实现适配。

## 决策依据

- 仓库已具备 no_std 的基础层（`crates/spark-uci`、`crates/spark-core`），且 L0 crate 已使用 `#![no_std]`。
- dataplane 已存在产品化 Options 雏形（`crates/spark-transport/src/config.rs` 的 `DataPlaneOptions`），并提供 mgmt HTTP profile。
- 已存在跨后端契约测试框架（`crates/spark-transport-contract`）与核心观测指标（`DataPlaneMetrics`）。
- mgmt plane 已在 transport 上 dogfooding（`crates/spark-ember/src/http1/transport_server.rs`）。

## 影响

- 里程碑与 PR gate 必须以 **Contract + Baseline + Dogfooding** 三件套之一作为交付证据。
- 任何功能扩展（新协议/新后端/新优化）必须先确保不会破坏上述三重硬指标。
