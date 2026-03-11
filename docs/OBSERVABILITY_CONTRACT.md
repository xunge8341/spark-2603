# Observability Contract（稳定口径）

本文件冻结 **证据事件（EvidenceEvent）** 与 **核心指标（DataPlaneMetrics）** 的“名称/字段口径”。

目标：
- **跨 OS/后端一致**（Windows IOCP / Linux epoll|io_uring / macOS kqueue / WASI/WASM）
- **可审计/可回归**（contract suite 与 perf gate 依赖这些口径）
- **避免字符串散落**：统一使用 `spark_uci::names` 常量

## Evidence Events（P0）

结构：`spark_uci::EvidenceEvent`

| name 常量 | 含义 | 关键字段 | unit_mapping |
|---|---|---|---|
| `BACKPRESSURE_ENTER` | 出站队列超过 high watermark，暂停读 | `pending_write_bytes` | `UNITMAP_BACKPRESSURE_BYTES_V1` |
| `BACKPRESSURE_EXIT` | 出站队列低于 low watermark，恢复读 | `pending_write_bytes` | `UNITMAP_BACKPRESSURE_BYTES_V1` |
| `DRAINING_ENTER` | 进入 draining（停止读，允许写 flush） | `pending_write_bytes`, `inflight` | `UNITMAP_DRAINING_V1` |
| `DRAINING_EXIT` | flush 完成后退出 draining | `inflight` | `UNITMAP_DRAINING_V1` |
| `DRAINING_TIMEOUT` | draining 超时强制关闭 | `pending_write_bytes`, `inflight` | `UNITMAP_DRAINING_V1` |
| `FLUSH_LIMITED` | flush hit fairness budget（公平性预算打满） | `pending_write_bytes` | `UNITMAP_FLUSH_LIMITED_V1` |
| `FRAME_TOO_LARGE` | inbound 超过 profile 限制（mgmt/http1/framing） | `value=size`, `pending_write_bytes=max` | `UNITMAP_FRAME_TOO_LARGE_V1` |

| `PEER_HALF_CLOSE` | peer half-close（read EOF），抑制 READ interest 但允许写 flush | `pending_write_bytes` | `UNITMAP_CLOSE_V1` |
| `CLOSE_REQUESTED` | 本地请求关闭（显式 close 或内部 shutdown） | `pending_write_bytes` | `UNITMAP_CLOSE_V1` |
| `CLOSE_COMPLETE` | 连接失活/资源回收完成（CloseComplete） | `reason`（requested/peer_half_close/reset/timeout/error/closed） | `UNITMAP_CLOSE_V1` |
| `ABORTIVE_CLOSE` | abortive close（RST/Reset）观察点 | `reason=reset` | `UNITMAP_CLOSE_V1` |

> 说明：`reason` 字段允许演进，但 **name 与 unit_mapping 必须稳定**。

## Core Metrics（P0 counters）

这些字段在 `spark_transport::DataPlaneMetrics` 中以 Atomic counter 形式存在；**名称单一事实来源为** `spark_uci::names::metrics`，exporter 只能引用常量并按需加前缀（例如 `spark_dp_`）。

- Connections
  - `accepted_total`, `closed_total`, `active_connections`
- IO
  - `read_bytes_total`, `write_bytes_total`
  - `write_syscalls_total`, `write_writev_calls_total`
- Backpressure / Draining
  - `backpressure_enter_total`, `backpressure_exit_total`
  - `draining_enter_total`, `draining_exit_total`, `draining_timeout_total`
- Decode / Limits
  - `decoded_msgs_total`, `decode_errors_total`, `inbound_frame_too_large_total`
- Fairness / Cumulation
  - `flush_limited_total`
  - `inbound_coalesce_total`, `inbound_copied_bytes_total`



## Driver kernel internals（P1 stable counters）

这些计数器用于解释 dataplane 内部调度/任务压力，**不改变用户可见语义**。
名称同样冻结在 `spark_uci::names::metrics`，exporter 输出时加 `spark_dp_` 前缀。

- Scheduler
  - `driver_schedule_interest_sync_total`
  - `driver_schedule_reclaim_total`
  - `driver_schedule_draining_flush_total`
  - `driver_schedule_write_kick_total`
  - `driver_schedule_flush_followup_total`
  - `driver_schedule_read_kick_total`
  - `driver_schedule_stale_skipped_total`
- Reactor register de-dup
  - `driver_interest_register_total`
  - `driver_interest_register_skipped_total`
- Task ownership
  - `driver_task_submit_total`
  - `driver_task_submit_failed_total`
  - `driver_task_submit_inflight_suppressed_total`
  - `driver_task_submit_paused_suppressed_total`
  - `driver_task_finish_total`
  - `driver_task_reclaim_total`
  - `driver_task_state_conflict_total`

> 说明：这些指标是“解释内部行为”的工具，属于 P1；核心 P0 contract 不依赖它们，但它们会显著提升性能回归与跨平台诊断可解释性。

## Derived Gauges（P0 gauges）

这些指标来自 `DataPlaneMetricsSnapshot::derive()`，并在 Prometheus exporter 中以 gauge 形式输出。
名称同样必须使用 `spark_uci::names::metrics` 常量。

- `write_syscalls_per_kib`
- `write_writev_share_ratio`
- `inbound_copy_bytes_per_read_byte`
- `inbound_coalesces_per_mib`

## Contract Gate

- `spark-transport-contract` 的 P0 suite 会断言关键 EvidenceEvent name。
- Perf gate 会用 `DataPlaneMetricsSnapshot::derive()` 输出派生指标（syscalls/KB, writev share, copy/byte）。

## Effective Config（P1: 审计快照 contract）

为避免多模块手工拼接文本，effective config 以稳定结构体为主 contract：

- transport：`DataPlaneConfig::describe_effective()` / `DataPlaneOptions::describe_effective()`
  - 输出 `DataPlaneEffectiveConfig`（bind/backlog/budget、watermark、flush policy、hard cap、framing、max_frame_hint）。
  - 若 framing 为 HTTP/1，则包含 `EffectiveHttpLimits { max_request_bytes, max_head_bytes, max_headers }`。
- host/mgmt：
  - `MgmtTransportProfileV1::describe_effective_config()` → `MgmtProfileEffectiveConfig`
  - `ServerConfig::describe_effective_config()` → 同时包含 default transport 与 perf overlay transport 的 effective 快照。

### Perf overlay 覆盖边界（冻结）

由 `DataPlaneConfig::perf_overlay_boundary()` 明确表达：

- 仅允许覆盖：`watermark`、`flush_policy`、`budget`、`emit_evidence_log`。
- 绝不覆盖：`bind`、`max_accept_per_tick(backlog)`、`max_channels`、`framing`、`drain_timeout`、`max_pending_write_bytes`。

> 说明：debug 文本格式不作为 contract；稳定 struct 才是序列化/比对基线。
