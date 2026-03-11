# Gap Status (Current vs North Star)

This document is the trunk baseline. It must match code and verification scripts.

> Baseline snapshot: 2026-03 (T0 freeze).

## Achieved (trunk)

### Driver scheduling kernel is explicit and reviewable
- `ScheduleReason` is frozen: `InterestSync / DrainingFlush / WriteKick / FlushFollowup / ReadKick / Reclaim`.
- Single-enqueue semantics and generation-aware stale-drop are in place.
- `install_channel() -> sync_interest(chan_id)` contract baseline is frozen.

### Safety/quality gates are executable
- `scripts/verify.sh` blocks on: fmt, clippy, deps invariants, panic scan, unsafe audit, workspace compile, contract suite, workspace tests.
- perf/bench/native-completion are optional explicit gates (`SPARK_VERIFY_*`).
- `scripts/unsafe_audit.sh` now matches real core unsafe footprint (`lease.rs`, `reactor/event_buf.rs`, `async_bridge/channel_state.rs`) and enforces `SAFETY` comments.

### RX lease path has landed Phase-A boundary
- Stream path can borrow token-backed bytes in-process (`with_stream_token`) with RAII token release.
- Datagram path still materializes to owned bytes by design.

## Remaining gaps (priority order)

### Gap 1 — RX zero-copy not fully closed-loop
**Now:** stream can borrow token-backed bytes during dispatch, but framing/cumulation still has copy paths; datagram remains owned.
**North Star:** stream RX zero-copy-first end-to-end, copy only for explicit retention/fallback with metrics.
**Next:** evaluate Phase-B only with evidence (owned-segment append or equivalent), keep ownership boundary auditable.

### Gap 2 — Multi-backend parity
**Now:** mio is production dataplane baseline; IOCP crate is Windows leaf boundary with phase-0 wrapper + native-completion prototype tests.
**North Star:** IOCP/epoll/kqueue/io_uring pass same P0 contracts.

### Gap 3 — CI/nightly hard gates
**Now:** perf/bench/completion are opt-in (not default blocking in `verify.sh`).
**North Star:** policy-driven default gates per platform with clear blocking rules.

### Gap 4 — Safety hardening depth
**Now:** panic-free scan + unsafe confinement + SAFETY comment checks are in place.
**North Star:** add fuzz/schedule-perturbation/loom/miri-sanitizer layers.

## Known blocking risk
- Windows mio `write_pressure_smoke` forward-progress stall remains open and is tracked in `docs/KNOWN_ISSUES.md`.

## Update 2026-03-11 (T1 unsafe 治理收敛)

- `async_bridge/channel_state.rs` 已移除 stream token 借用 fast-path，统一走 `materialize_rx_token`，消除了该文件中的全部 `unsafe`。
- `unsafe` 治理从“限定模块”升级为“全 crates 台账 + 脚本同步”机制：`docs/UNSAFE_REGISTRY.md` + `scripts/unsafe_audit.sh`。
- `unsafe_audit.sh` 现强制：
  - 每个 `unsafe` 前必须有 `SAFETY:` 注释；
  - `crates/` 中每个含 `unsafe` 文件必须在台账中登记；
  - 台账不能有失效条目（代码已无 unsafe 但文档仍保留）。
