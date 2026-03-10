# Spark 通用通信基础设施：North Star（终态定义）

## 1. 产品定位（工程可验收语言）

Spark 的终态定位是：

> **不限协议、跨 OS/CPU、可扩展到 Embedded/WASM 的通用通信基础设施**，以 **C++ 性能、Rust 安全、ASP.NET Core 体验** 为同时达成的硬目标，满足工业 / 金融 / 军事等生产环境的性能与安全要求。

### 1.1 C++ 级性能（可验收）

- **热路径可控分配**：默认配置下，数据面（dataplane）收发热路径不产生不可控堆分配；若存在分配，必须被 Options/Policy 显式预算约束，并具备可观测计量。
- **零拷贝优先**：payload 以分段/切片形态流转，避免拼接再拷贝；copy 必须可计量（copy/byte、copied_bytes）。
- **批量 syscall**：发送侧优先 vectored write（Linux `writev` / Windows `WSASend`），并有稳定指标（syscalls/KB、writev_share）。
- **尾延迟稳定**：提供统一压测与基线（吞吐、p99/p999、内存峰值、syscall/byte、copy/byte、context switches），并具备回归门禁。

### 1.2 Rust 级安全（可验收）

- **unsafe 收敛且可审计**：核心层（L0/L1/L2）默认 `forbid(unsafe_code)`；如必须存在 `unsafe`，必须集中在少数可审计模块并具备 Safety Comment 与台账。
- **panic-free 默认语义**：库侧默认不 panic（非测试代码禁止 `unwrap/expect/panic!/todo!/unimplemented!`）；错误通过稳定错误码体系外显。
- **并发语义可证明/可测**：关键状态机具备 deterministic schedule tests（或 loom 覆盖关键路径），并冻结 close/half-close/timeout/backpressure/flush 的契约集。

### 1.3 ASP.NET Core 级体验（可验收）

- **Options 化装配**：80% 用户只需配置 3～5 个字段即可上线（强默认、少参数、清晰错误）。
- **可观测性一等公民**：metrics/events/logs 口径稳定（事件名、字段、含义冻结），并作为契约的一部分。
- **内部生态自举（Dogfooding）**：控制面/管理面（HTTP）优先运行在 Spark 之上，复用 dataplane 的 framing/error/evidence/metrics，形成闭环。

## 2. 平台适配与可移植性

- **后端可插拔**：Windows(IOCP)、Linux(epoll/io_uring)、macOS(kqueue) 均通过 L3 backend 适配，且同一语义 contract suite 必须一致通过。
- **扩展目标**：支持 Embedded（无 OS 或 RTOS）与 WASM（WASI 网络）环境；核心语义不得绑定 OS。

## 3. 终态架构分层（稳定边界）

- **L0（no_std）**：错误码、证据事件 schema、时间/指数退避、ByteOrder、Varint。
- **L1（alloc）**：Bytes/BytesMut/ByteQueue、scan、framing primitives（Line/Delimiter/LengthField/Varint32）。
- **L2（std）**：Transport state machine（channel/driver/pipeline）、Backpressure/flush fairness、timeout、close semantics。
- **L3（backend）**：mio/epoll/io_uring/IOCP/kqueue 适配。
- **L4（protocol stacks）**：HTTP/1（最小但完整）、SIP/Text（已有），后续 QUIC/TLS（可选）。
- **L5（hosting）**：HostBuilder/Routes/Diagnostics，统一 Options 口径。

## 4. 终态验收方式（永远以“证据”交付）

- **Contract**：跨后端一致性契约（P0 必过 + P1 扩展）。
- **Baseline**：性能/资源基线与自动回归门禁。
- **Dogfooding**：管理面与内部服务运行在 Spark 上，且口径统一。
