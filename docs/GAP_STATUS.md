# Gap Status (Current vs North Star)

This document is the trunk baseline. It must match code and verification scripts.

> Baseline snapshot: 2026-03 (T2 Windows/IOCP status honesty + forward-progress gate).

## Achieved (trunk)

### Driver scheduling kernel is explicit and reviewable
- `ScheduleReason` is frozen: `InterestSync / DrainingFlush / WriteKick / FlushFollowup / ReadKick / Reclaim`.
- Single-enqueue semantics and generation-aware stale-drop are in place.
- `install_channel() -> sync_interest(chan_id)` contract baseline is frozen.

### Safety/quality gates are executable
- `scripts/verify.sh` blocks on: fmt, clippy, deps invariants, panic scan, unsafe audit, workspace compile, contract suite, workspace tests.
- perf/bench/native-completion are optional explicit gates (`SPARK_VERIFY_*`).
- verify output now prints Linux status / Windows status / known gaps explicitly.

### Windows backend status is now explicit and unified
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
**Now:** perf/bench/completion are opt-in (not default blocking in `verify.sh`).
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

## Update T4（性能证据与 gate 补齐）
- perf gate 从单点 `SPARK_PERF` 解析升级为“场景矩阵 + 基线对比”：`scripts/perf_gate.sh` 调用 `scripts/perf_report.sh` 生成 JSON/CSV，再按 `perf/baselines/perf_gate_*.json` 对比。
- 指标口径覆盖吞吐 + 尾延迟 + syscall/batching + copy + backpressure，并补充 `peak_rss_kib` 与 `peak_inflight_buffer_bytes`。
- 基线按 Unix/Windows 分离，显式避免“Linux 最优数字外推所有平台”。
