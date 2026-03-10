# ADR-004 Contract Suites（P0 必过 / P1 扩展）

## 决策

将 transport 语义按“运营可验收”拆成两级契约：

- **P0（必过）**：任何后端 / 任何优化都不得破坏；用于跨平台一致性与生产可运维性。
- **P1（扩展）**：覆盖更深的边界行为与压力场景；用于兑现“世界一流”尾延迟与鲁棒性。

## P0 契约范围（必须在 `spark-transport-contract` 中体现）

1) **Backpressure**：HWM/LWM 驱动 PauseRead/ResumeRead，writability 事件与指标一致。
2) **Flush fairness**：预算（bytes/syscalls）固定且可观测；不会饿死其他连接；writable 事件缺失也能推进（reactor 差异吸收）。
3) **Close/Draining**：close/abort/timeout 语义稳定；close 幂等；draining 超时强制关闭并出证据。
4) **Partial write**：短写/WouldBlock 下的队列保序、interest 更新、重试语义。
5) **Error mapping**：平台错误映射到稳定 `KernelError` 集合，并可被 contract 断言。

## P1 契约范围（里程碑逐步纳入）

- **Half-close**：读半关/写半关的证据事件与状态机一致。
- **Fuzz-able state machine**：随机时序（read/write/flush/close/timeout）不崩溃、不泄漏、契约不破。
- **Resource ceilings**：极端输入下的内存峰值、copy/byte、syscalls/byte 有上限且可回归。

## 决策依据

- 当前仓库已具备 P0 suite 雏形（`crates/spark-transport-contract/src/suite.rs`），并已在 dataplane 内部形成可观测指标（`DataPlaneMetrics`）。
- flush fairness 的“前进性”由 driver-level contract 明确约束（`crates/spark-transport-contract/tests/flush_limited_progress.rs`），防止未来后端/优化回归。
- 多后端（IOCP/epoll/kqueue/WASI）推进时，最容易跑偏的是 flush/close/partial write 等边界语义；先冻结 P0 可避免后续成倍返工。

## 影响

- 任何新增后端必须先过 P0；任何性能优化 PR 必须更新/新增至少一条契约或基线证据。
