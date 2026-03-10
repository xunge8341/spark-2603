# ADR-000 Driver-Abstraction Pattern（Rust 生态式积木设计）

## 决策

Spark 采用 **Driver-Abstraction Pattern**：

1. `spark-transport` 只定义抽象与语义契约（traits + 状态机 + contract tests）。
2. 后端实现（mio/uring/serial/...）在叶子层 crate 实现这些 trait。
3. 热路径默认走静态分发（泛型/单态化），边界处才使用 `dyn Trait`。

## 约束（PR Gate）

- **核心层禁止第三方 runtime 依赖**：`spark-transport` / `spark-host` / `spark-ember` 不得依赖 mio/tokio/bytes/log。
- **热路径禁止 downcast**：tick/flush/read loop 内不得出现 `Any::downcast_*`。
- `dyn Trait` 只能出现在边界（mgmt 或插件层），不得贯穿 dataplane 热路径。

## 取舍

- 该模式能最大化保持核心闭源的可定制性（第三方实现 trait）并维持零开销组合。
- 若需要“运行时加载插件”，Rust 缺少稳定 ABI，必须走 C ABI/WASM 等边界，会引入开销；此为另一个 ADR 讨论范围。

