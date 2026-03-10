# DotNetty 经验摘录：把 Netty 抽象带到另一种语言运行时

本项目的目标是“像 Netty 一样把路径和抽象理顺”，同时遵守 Rust 的所有权/生命周期/零成本抽象的工程约束。
DotNetty 的移植经验对我们很有参考价值，因为它同样面对“语言运行时差异”带来的结构性取舍。

## 1. Pipeline 语义比实现细节更重要

DotNetty 的核心成功点不是“照搬 Java 的类层次”，而是保持了：

- **Inbound/Outbound 方向语义**（事件从 head→tail / tail→head）
- **write/flush 分离**（write 入队，flush drain）
- **writability（水位线）驱动 backpressure**（高/低水位切换 + 事件通知）

Rust 侧建议：先把这些语义骨架立住，再逐步优化为零拷贝/池化。

## 2. Buffer/内存策略必须与语言生态契合

DotNetty 依赖 `IByteBuffer` 与池化 allocator，在 GC 语言里减少分配与拷贝。
Rust 侧的等价策略通常是：

- `Bytes/BytesMut`（引用计数 + slice/clone cheap）
- 自研 slab/arena + lease（需要非常谨慎的安全封装）
- 对 async 边界保持“owned”数据（避免跨 await 借用）

因此 bring-up 阶段允许 materialize（复制），但**必须保持 API 语义不变**，以便后续替换为池化/零拷贝。

## 3. EventLoop/线程模型要“说清楚”

Netty/DotNetty 的一个隐含契约是：

- 同一 channel 的 handler 回调，默认在同一个 event loop 线程执行（避免锁）

Rust 侧如果引入 tokio/多线程执行器，要么：

- 把 channel 逻辑固定在单线程 driver（推荐，语义最一致）
- 或者明确哪些 handler 是 `Send + Sync`，哪些必须在 event loop 线程内运行

## 4. Completion IO（io_uring）与 readiness IO（mio）的统一策略

DotNetty 没有 io_uring，但它的移植经验提示我们：

- **不要让 IO 模型差异污染 pipeline/codec**
- 在内部保留一条明确的 IO 原语边界（类似 Netty 的 `Channel.Unsafe`）

Rust 侧推荐把 mio/uring 差异限制在 `Channel` 内部的 IO backend 与 driver glue 中。
