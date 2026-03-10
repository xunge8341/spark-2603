# 给 OOP 背景同学的 Rust 快速对照表（Spark 项目版）

> 目标：让不熟 Rust 的同学理解“同等方案是什么”，避免把 OOP 的继承/容器/反射硬搬进来。

## 1. 所有权/借用：Rust 的第一性原理

在 Rust 里，**谁拥有（own）谁负责释放**，并且通过编译器强制：

- `T`：拥有值（会移动 move）
- `&T`：共享借用（只读）
- `&mut T`：可变借用（独占）

**常见 OOP 误区**：把 `&T` 当成“引用”随便拿字段出去用。

- OOP：从引用里拿字段/替换字段很常见
- Rust：从 `&T` 里**不能 move 出字段**（否则原对象被挖空）

对应方案：
- 启动期配置：`clone()`（一次性，非 hot path）
- 热路径数据：用借用 `&[u8]` / `&mut [u8]` 或者 `Bytes` 这种轻量共享视图

## 2. 继承/虚函数 vs Traits + 组合（Composition）

OOP 常用：
- 继承扩展行为
- 虚函数实现多态

Rust 推荐：
- **trait 表达能力**，struct 负责数据
- **组合**组织系统（builder/layer/router）

性能关键：
- 热路径优先 **泛型（static dispatch）** → 编译期单态化 → 零开销
- 边界处才用 `dyn Trait`（dynamic dispatch）

在 Spark 中：
- 数据面（dataplane）尽量避免 `Arc<dyn ...>` 贯穿热路径
- 控制面（mgmt）QPS 低，`dyn` 更可接受

## 3. “运行时 DI 容器/反射”在 Rust 的等价方案

Rust 很少做运行时反射扫描；更常见的是：

- `Builder` 在编译期把类型拼装好
- 用 trait bound 约束能力

Spark 的 `HostBuilder::pipeline(|p| ...)` 就是这条路：
- 类似 ASP.NET Core 的装配体验
- 但组合发生在编译期，避免运行时容器带来的性能与复杂度

## 4. 插件化/生态化：Driver-Abstraction Pattern

想实现：
- 核心源码不开放
- 第三方可定制
- 性能零损耗

Rust 社区常用做法是：

- 核心 crate 提供 **抽象（traits + 状态机 + 语义契约）**
- 后端 crate 实现这些抽象（mio/uring/serial/...）
- 热路径通过泛型组合 → 单态化 → 零损耗

类似：`embedded-hal` / `sqlx`。

在 Spark 中：
- `spark-transport`：抽象与语义（runtime-neutral）
- `spark-transport-mio`：mio 后端实现（叶子层）
- `spark-ember`：默认 server（Kestrel-like，std-only）

