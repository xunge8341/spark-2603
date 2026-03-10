# Code Style & Architecture Cleanliness（对标 Microsoft 工程风格）

> 目标：代码要做到“可读、可审计、可长期维护”，并与 Spark 的分层架构一致。

## 1. 总原则

- **Clarity over cleverness**：宁可写直白代码，也不要用技巧换取短代码。
- **Small surface area**：公共 API 面越小越好；一旦公开就要稳定、可版本化。
- **Layering first**：分层边界比局部优化更重要；不允许跨层捷径。
- **No intermediate state**：禁止长期别名/双实现/过渡层（见 ADR-003）。

## 2. 模块组织

- 一个抽象/状态机尽量一个文件；文件名与类型名对应。
- `lib.rs` 只做模块声明与稳定 re-export；不堆放实现细节。
- 核心层（L0/L1/L2）不依赖具体后端与外部 runtime；后端与导出器都下沉到 leaf crates。

## 3. 错误与证据

- 错误必须进入稳定错误码体系：优先 `KernelError` + `internal_code(subsystem, detail)`。
- EvidenceEvent/metrics/events 的 **名字与字段**一旦冻结，不随意改动；变更必须版本化并更新文档。

## 4. panic-free / unsafe

- 非测试代码禁止 `unwrap/expect/panic!/todo!/unimplemented!`。
- `unsafe` 必须集中到可审计模块，并提供 Safety Comment；核心层默认 `forbid(unsafe_code)`。

## 5. PR 交付证据

每个 PR 必须至少提供以下之一：

- **Contract**：新增/加固契约测试（跨后端一致性）。
- **Baseline**：新增/更新性能基线与回归门禁。
- **Dogfooding**：管理面/内部服务在 Spark 上跑通，并证明口径一致。

同时必须满足：`fmt + clippy -D warnings + architectural invariants`。

## 6. Deterministic builds / supply-chain

- 所有 gate 脚本默认使用 `--locked`：CI/本地验证不得隐式改写 `Cargo.lock`（变更必须显式提交）。
- Nightly 运行 `cargo deny check`（licenses/bans/sources/advisories），把依赖/许可证/漏洞作为工程门禁的一部分。
