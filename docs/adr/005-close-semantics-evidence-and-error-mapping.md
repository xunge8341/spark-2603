# ADR-005: Close / Half-close / Abort semantics are P0 and must be observable

- Status: Accepted
- Date: 2026-03-02

## Context

跨 OS/后端（IOCP/epoll/kqueue/WASI）在 close/half-close/abort（RST/Reset）语义上存在天然差异：

- 有的后端依赖事件（Readable/Writable/Closed）才能推进；
- 有的后端在 interest toggle（PauseRead/ResumeRead）后可能丢事件；
- Reset/Timeout 等错误在不同平台的映射与表现不同。

在工业/金融/军事生产环境中，close 语义必须满足：

- **确定性**：不允许出现“连接僵死”（既不读也不写但资源不回收）。
- **可审计**：必须能从证据事件中看出 close 的原因（requested/peer_half_close/reset/timeout/error）。
- **可回归**：contract suite 能稳定捕获语义回退。

## Decision

1) 冻结 close 相关证据事件命名（`spark_uci::names::evidence`）：

- `PEER_HALF_CLOSE`
- `CLOSE_REQUESTED`
- `CLOSE_COMPLETE`
- `ABORTIVE_CLOSE`
- `UNITMAP_CLOSE_V1`

2) 语义层引入 best-effort 的 `CloseCause`（无分配、no_std 友好）：

- 由 `ChannelState` 维护，用于 `CloseComplete` 的稳定 reason。
- `CloseComplete` 在 `ChannelInactive`（资源回收完成）时发出，形成证据闭环。

3) driver 在遇到任意 I/O error 时必须 **确定性 request_close**：

- 目的：避免“等待后端 Closed 事件”造成的僵尸连接。
- 要求：panic-free、bounded、不开启 busy loop。

4) 契约化：新增 driver 级 contract tests 覆盖显式 close 与 abortive close。

## Consequences

- 正向：
  - close/half-close/abort 的语义与口径在所有后端一致，便于 dogfooding 与运维。
  - contract suite 能第一时间阻止回退，降低后续引入 IOCP/epoll/kqueue/WASI 的风险。

- 代价：
  - driver 在 error 路径会主动 request_close（best-effort）。
  - 需要维护 `CloseCause` 的稳定映射（reason 字段允许扩展，但事件名与 unit_mapping 必须稳定）。

## Alternatives considered

- 仅依赖后端 Closed 事件：被拒绝（跨平台不可靠，易产生僵尸连接）。
- 用字符串动态构造 reason：被拒绝（分配/格式化会污染热路径且不利于 no_std/embedded）。
