# Gap Status (Current vs North Star)

This document is the trunk baseline. It must match code and verification scripts.

> Baseline snapshot: 2026-03 (T2 Windows/IOCP status honesty + forward-progress gate).

## Achieved (trunk)

### Driver scheduling kernel is explicit and reviewable
- `ScheduleReason` is frozen: `InterestSync / DrainingFlush / WriteKick / FlushFollowup / ReadKick / Reclaim`.
- Single-enqueue semantics and generation-aware stale-drop are in place.
- `install_channel() -> sync_interest(chan_id)` contract baseline is frozen.

### Safety/quality gates are executable
- `scripts/verify.sh` / `scripts/verify.ps1` blocks on: fmt, clippy, deps invariants, panic scan, unsafe audit（registry-synced）, workspace compile, contract suite, workspace tests.
- perf/bench/native-completion are optional explicit gates (`SPARK_VERIFY_*`).
- verify output now prints Linux status / Windows status / known gaps explicitly.

### Windows backend status is now explicit and unified
- unsafe inventory is unified by `docs/UNSAFE_REGISTRY.md` (single source of truth), with shell + PowerShell audits enforcing documented `SAFETY` and registry bidirectional consistency.
- `spark-transport-iocp` is classified as a **phase-0 compatibility layer**, not a native IOCP dataplane.
- Default dataplane is wrapper mode (delegates readiness engine) and remains **not production-ready as native IOCP**.
- Native completion remains an opt-in prototype (`--features native-completion`) for phase-x validation.

## Remaining gaps (priority order)

### Gap 1 — RX zero-copy not fully closed-loop
**Now:** stream can borrow token-backed bytes during dispatch, but framing/cumulation still has copy paths; datagram remains owned.
**North Star:** stream RX zero-copy-first end-to-end, copy only for explicit retention/fallback with metrics.
**Next:** evaluate Phase-B only with evidence (owned-segment append or equivalent), keep ownership boundary auditable.

### Gap 2 — Multi-backend parity
**Now:** mio is production dataplane baseline; IOCP is phase-0 compatibility layer + partial native-completion prototype.
**North Star:** IOCP/epoll/kqueue/io_uring pass same P0 contracts.

### Gap 3 — CI/nightly hard gates
**Now:** day-time `verify.*` keeps perf/bench/completion opt-in, while `verify_nightly.*` enables perf/bench/completion/no_std by default.
**North Star:** policy-driven default gates per platform with clear blocking rules.

### Gap 4 — Safety hardening depth
**Now:** panic-free scan + unsafe confinement + SAFETY comment checks are in place.
**North Star:** add fuzz/schedule-perturbation/loom/miri-sanitizer layers.

## Known blocking risk
- Windows mio `write_pressure_smoke` forward-progress stall remains open and is tracked in `docs/KNOWN_ISSUES.md`.
- This issue is now surfaced as an explicit known-failing gate/report item (no silent ignore policy).

## Update T3（超时/就绪/排水治理）
- 已在 host/ember 管理面补齐连接级 timeout（idle/read/write/headers）与请求级 timeout（默认 + route/group 覆盖）。
- `/healthz` 与 `/readyz` 语义分离：`/readyz` 现在同时检查 draining、listener readiness、dependency readiness。
- 增加过载治理配置：并发上限、每连接 inflight、队列上限与 reject policy（503/429/直接关闭）。
- transport-backed 管理面主路径已接入上述治理：draining 拒绝新请求、route/group/default request timeout 返回 504、全局并发与队列过载按 reject policy 生效。

## Update T4（性能证据与 gate 补齐）
- perf gate 从单点 `SPARK_PERF` 解析升级为“场景矩阵 + 基线对比”：`scripts/perf_gate.sh` 调用 `scripts/perf_report.sh` 生成 JSON/CSV，再按 `perf/baselines/perf_gate_*.json` 对比。
- 指标口径覆盖吞吐 + 尾延迟 + syscall/batching + copy + backpressure，并补充 `peak_rss_kib` 与 `peak_inflight_buffer_bytes`。
- 基线按 Unix/Windows 分离，显式避免“Linux 最优数字外推所有平台”。
- PowerShell perf gate 已与 shell 口径统一：同样走 `perf_report` JSON/CSV 合同，并按 scenario/global 双层阈值阻断。
- nightly 路径（`verify_nightly.sh/.ps1`）默认开启 perf gate，普通 verify 继续保持可选。

## Update T5（主干并发与过载治理）
- `AppServiceHandler` 从“单 inflight + 无上界 queue”升级为可配置模型：`max_inflight_per_connection` + `max_queue_per_connection` + `overload_action`（FailFast / Backpressure / CloseConnection）。
- 默认值明确：`max_inflight_per_connection=1`、`max_queue_per_connection=1024`、`overload_action=FailFast`，确保不出现无限队列。
- 数据面指标已闭环 overload 可观测：新增 `overload_reject_total` / `overload_backpressure_total` / `overload_close_total` 与 `app_queue_high_watermark`，并在 `/metrics` 导出。
- 新增单测覆盖：队列上限、背压信号传播、拒绝关闭语义。

## Update T6（扩展点整理）
- pipeline builder 暴露 `app_service_options(...)` 作为 per-handler/per-protocol 覆盖入口（不引入额外多层 trait）。
- dataplane 配置新增并文档化 app-service 并发/队列/过载策略字段，并通过 `describe_effective()` 对外可审计。
- pipeline 对外 re-export 收敛到最小稳定入口（builder/handler/options/profile），并明确 internal module 非稳定承诺。
- 新增 `docs/EXTENSION_SURFACE.md`，固定连接/请求/metrics-tracing/codec-framing 的扩展边界与稳定承诺。
- `spark-dist-mio` 新增最小示例：`mgmt_metrics_hook.rs` 与 `mgmt_timeout_group.rs`，分别覆盖 diagnostics hook 与 group timeout 用法。


## Update T7（drain/readiness 生命周期闭环）
- `EmberState` 新增轻量状态面：`accepting_new_requests`、`active_requests`、`overloaded`，并保持 `draining/listener_ready/dependencies_ready` 组合可观测。
- `/readyz` 语义收敛为：非 draining、可接收新请求、listener ready、dependencies ready、非 overloaded。
- `/drain` 不再只是切标志：std/transport server 均在 drain 后拒绝新请求，并对在途请求按 request timeout deadline 收敛。
- 边界说明：当前依赖就绪仍是布尔聚合，不引入外部依赖探针框架。

## Update T7.1（transport-backed 生命周期回落补齐）
- transport-backed 管理面在 `try_spawn_with(_perf)` 成功后，保持与 std server 一致的启动状态：`listener_ready=true`、`accepting_new_requests=true`、`overloaded=false`。
- 新增 transport handle join wrapper：当 transport dataplane 线程退出（正常/panic）时，统一回落 `listener_ready=false`、`accepting_new_requests=false`、`overloaded=false`，避免“只在启动置 true，无退出回落”的状态漂移。
- `active_requests` 继续由请求级 guard 维护；单测补齐“请求结束回到 0、重复 decrement 不下溢”约束，确保 drain/退出期间不出现负数语义或泄漏。
