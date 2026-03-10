# 联合攻坚组：Netty / DotNetty / ASP.NET Core / Rust / Tokio 方向改造纪要

> 目标：把 **core path**（channel → codec → pipeline → outbound flush/backpressure）理顺成“Netty/DotNetty 语义一致”的骨架，同时保留 Rust 的编译期选择与零开销抽象能力。

## 0. 红线（North Star 对齐）

- 定位：**C++ 性能 + Rust 安全 + ASP.NET Core 体验** 的跨软硬件平台通用通信基础设施（不限协议），面向工业/金融/军事等生产环境。
- 可扩展：架构需可扩展到 **Embedded / WASM**（核心语义不绑定 OS）。
- 以终为始：里程碑以理想终态反推差距；每次迭代必须输出 **决策依据**（contract / baseline / dogfooding 证据）。
- 将军赶路：只抓主干大石头（语义契约冻结、性能/安全门禁、自举闭环），避免过早陷入细节。
- 代码风格：对标 Microsoft 工程风格，强调清洁分层与可维护性；**禁止兼容中间态**（alias/双实现）。


## 1. 参与角色（虚拟评审席位）

为便于分工与决策记录，定义以下“专家席位/owner”，用于后续 issue/PR 的责任归属：

- **Netty Channel/Pipeline 专家**：抽象边界、事件方向（inbound/outbound）、writability/flush 语义对齐
- **DotNetty 专家**：.NET 生态对齐（Kestrel 风格连接循环、可观测性、背压指标）
- **ASP.NET Core / Kestrel 专家**：连接生命周期、管道化处理（PipeReader/PipeWriter 类比）、服务化模型
- **Rust 语言专家**：`no_std`/`alloc` 边界、sealed trait、零拷贝安全封装、泛型隐藏（type alias + cfg）
- **Tokio/io_uring 专家**：readiness vs completion 模型差异、buffer pinning、后端替换策略

> 注：本仓库当前为 bring-up grade；专家席位用于约束改造方向与接口稳定性。

## 2. 决策原则（与 Netty/DotNetty 对齐，但不照搬对象模型）

1) **业务只写 codec/handler**：业务层不接触 reactor/interest/socket。

2) **写路径工程化**：
   - `write()` 入队（OutboundBuffer）
   - `flush()` drain（非阻塞写，必要时注册 WRITE interest）
   - 高/低水位线驱动 `is_writable` 与 `writability_changed` 事件/指标

3) **I/O 后端隔离**：
   - 对外只暴露语义 Channel（Netty 风格）
   - 对内保留 I/O 原语边界（类比 Netty `Channel.Unsafe`），并用 **编译期选择**（cfg/type alias）隐藏泛型复杂度

4) **代码结构规范**：
   - `lib.rs` 仅放模块声明与 API re-export
   - 一个抽象对象/状态机尽量一个文件（降低耦合与 review 成本）

## 3. 本轮已落地改造（2026-02-25）

### 3.1 IO trait 增强（为 writev/flush 语义铺路）

- `spark-io::IoOps` 增加 `try_write_vectored(&[&[u8]])`（默认回退到循环 `try_write`）
- `spark-transport-tcp::TcpChannel` 覆盖实现，使用 `std::io::Write::write_vectored`

### 3.2 Netty-style OutboundBuffer（写路径核心语义）

- 新增 `spark-adapter::async_bridge::OutboundBuffer`
  - `write()` 仅入队
  - `flush()` drain
  - 高/低水位线驱动 writability

### 3.3 async_bridge 模块“一个抽象一个文件”重排

- `async_bridge/mod.rs` 仅做导出
- `channel_driver.rs`：事件循环驱动 + 任务编排
- `dyn_channel.rs`：bring-up 的 object-safe IO wrapper（长期会被真正的 ChannelCore 取代）
- `outbound_buffer.rs`：OutboundBuffer + watermarks

## 4. 下一步（核心路径继续对齐 Netty）

1) **TCP cumulation + ByteToMessageDecoder loop**：把半包/粘包从业务抽离。
2) **ChannelPipeline（inbound/outbound 方向语义）**：让 bridge 变薄，handler 链成为业务唯一接触面。
3) **编译期后端选择落地**：mio/uring 使用 cfg + type alias 隐藏泛型，外部只见 `TcpChannel`。

### 3.4 ChannelPipeline “大刀阔斧”落地（refactor5）

- 将 per-connection 语义收敛到 `Channel<A>`：
  - `ChannelState`（IO + outbound buffer + backpressure）
  - `ChannelPipeline`（handler 链，inbound/outbound 双向语义）
- driver (`ChannelDriver`) 退回为纯 EventLoop：
  - poll reactor -> dispatch -> poll app futures -> flush outbound -> sync interest
- pipeline 支持 Netty/DotNetty 关键事件：
  - `channelActive / channelInactive`
  - `channelReadRaw -> (decoder) -> channelRead`
  - `channelReadComplete`
  - `exceptionCaught`
  - `channelWritabilityChanged`（由 watermarks 驱动）
- DotNetty 经验落地：pipeline 维持同步接口；异步业务由 `AppServiceHandler` 生成 future，driver 统一 poll。
