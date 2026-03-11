# Gap Status (Current vs North Star)

This document is the *single* living view of "where we are" vs the North Star.
It is intentionally short and **trunk-focused**（将军赶路不打小鬼）.

> 基线：BigStep-28 Step-5 + contract 修正后全绿（`scripts/verify.ps1` + contract suite）。

## Achieved (trunk)

### Driver scheduling kernel is explicit and reviewable
- Driver work reasons are explicit and stable (`ScheduleReason`):
  `InterestSync / DrainingFlush / WriteKick / FlushFollowup / ReadKick / Reclaim`.
- Single-enqueue semantics are frozen per `(chan_id, reason)` via generation-aware slot state.
- Stale queue items are dropped **before** consuming bounded per-tick work budget.
- One-task-per-channel ownership is frozen via generation-aware slot-local `TaskSlotState`.
- Interest synchronization is edge-triggered with per-slot register de-dup (`InterestSlotState`).
- Contract fix: `install_channel()` establishes an initial interest register baseline to satisfy
  draining/ordering contracts under edge-triggered interest sync.

### Observability contract is frozen (plus driver-kernel internals)
- Evidence + metric names are centralized in `spark-uci::names`.
- Driver-kernel internal counters exist and are exported as `spark_dp_*`:
  scheduler pressure, stale-skips, interest register de-dup, task ownership pressure.

### Symmetric framing (TX/RX) and close semantics are hardened
- Outbound vectored frames (`OutboundFrame`) with bounded segments, partial-write progress covered.
- Close / half-close / abort evidence mapping is P0 gated.
- Flush fairness budget and iovec caps are productized and clamped.

## Remaining gaps (ordered by trunk priority)

### Gap 1 — RX zero-copy is not closed-loop yet (**current focus**) 
**Now:** lease/token exists, but token path is still materialized into owned bytes and then copied again into stream cumulation.
**North Star:** stream RX is zero-copy first; copying is reserved for explicit retention/fallback paths and backed by metrics.
**Next action:** close the RX loop in two stages:
1) **Phase A 收口**：lease token -> borrowed slice fast-path + RAII release，终态为 1 次 copy（cumulation tail）；
2) 之后再评估是否进入 Phase B（owned-segment append）以冲击 0-copy。

**Latest contract hardening (BigStep-29A.2):**
- Added regression tests for Phase A boundaries: token release exactly-once across success/decode-error/early-return/handler-error.
- Added tests that freeze borrowed-vs-owned boundary: stream can be borrowed; datagram must be owned.
- Added tests that freeze pre-frame fallback behavior: when `add_first` handlers exist, borrowed fast-path is bypassed.

### Gap 2 — “可控分配”证据闭环已接通，后续评估是否需要容器替换
**Now:**
- outbound 已有单点 hard cap（`OutboundBuffer::enqueue`）与默认兼容值（`usize::MAX`）；
- 已暴露 `VecDeque` growth/peak 与 cumulation tail growth/peak 证据；
- perf baseline 已输出 alloc evidence 摘要字段。
**North Star:** hot-path allocations are either avoided or explicitly budgeted (hard cap) with observable counters.
**Next action:**
- 用 baseline 数据观察是否需要 ring/pooling（仅在证据支持时推进）；
- 若推进，维持单点 budget 检查与配置单一归一化路径。

### Gap 3 — ASP.NET Core experience: effective config explainability remains
**Now:** `ServerConfig` 已收口为 `mgmt: MgmtTransportProfileV1`，管理面不再双轨保存。
**North Star:** Options/Profile is the only user-facing source of truth; effective config is explainable and auditable.
**Status update (BigStep-30A.2):**
- 已落地结构化 effective config：
  - `DataPlaneConfig::describe_effective()` / `DataPlaneOptions::describe_effective()`;
  - `MgmtTransportProfileV1::describe_effective_config()`;
  - `ServerConfig::describe_effective_config()`（含 default 与 perf overlay 生效值）。
- 已冻结 perf overlay 覆盖边界：`DataPlaneConfig::perf_overlay_boundary()`。

**Next action:** 将上述结构化快照接入启动日志与 healthz 可选输出开关（仅展示层，非新配置源）。

### Gap 4 — Multi-backend parity (largest platform gap)
**Now:** mio is primary; IOCP has runnable posted-packet closure, but true socket overlapped submission is pending.
**North Star:** IOCP/epoll/io_uring/kqueue all pass the same contract suite.

### Gap 5 — Perf baseline + automated regression gate
**Now:** local scripts + derived metrics exist.
**North Star:** CI/Nightly gates enforce thresholds (throughput, p99/p999, syscalls/KB, copy/byte, memory).

### Gap 6 — Safety hardening automation
**Now:** panic-free gates + unsafe policy exist.
**North Star:** fuzz + deterministic schedule tests/loom + optional miri/sanitizers.

### Gap 7 — Mgmt isolation proof under load
**Now:** mgmt dogfooding works; isolation policy exists.
**North Star:** repeatable smoke/bench proves mgmt does not degrade dataplane under concurrency.

## Known issues (non-blocking)
- Windows/mio `write_pressure_smoke` remains tracked in `docs/KNOWN_ISSUES.md`.
