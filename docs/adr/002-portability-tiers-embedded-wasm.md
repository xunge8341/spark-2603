# ADR-002 Portability Tiers（Embedded / WASM 扩展承诺）

## 决策

Spark 采用“分层可移植性承诺”（Portability Tiers），以保证可扩展到 Embedded 与 WASM，而不会牺牲主干语义一致性。

### Tier 定义

- **Tier-0（pure no_std）**：L0 必须可在 `#![no_std]` 环境编译，通过最小单元测试（若平台允许）。
- **Tier-1（alloc, no OS）**：L1 允许 `alloc`，但不得依赖线程/文件/网络；为嵌入式协议栈/驱动预留 backend 插槽。
- **Tier-2（std, OS backend）**：L2+L3 依赖 `std`，提供 Windows(IOCP)/Linux(epoll/io_uring)/macOS(kqueue) 后端。
- **Tier-3（WASM）**：目标 `wasm32-wasi`（WASI sockets），语义由同一 contract suite 约束。

### 边界约束

- 核心语义（L0/L1/L2）不得泄漏 OS 差异；OS 差异由 L3 backend absorb。
- contract suite 必须以“语义合同”为准，而非以“某后端行为”为准。

## 决策依据

- 现有 L0 crate（`spark-uci`）已是 no_std，且包含错误码与 EvidenceEvent schema，是跨平台语义的正确锚点。
- 现有 driver/channel/pipeline 结构已将 reactor/backend 与语义对象隔离（参见 ADR-000）。

## 影响

- 引入新后端（IOCP/epoll/kqueue/WASI）必须优先跑通 contract suite，而非先追性能。
- 若某个优化需要 OS 特性（例如 io_uring），必须以 feature/leaf crate 形式下沉到 L3，不得污染 L2。
