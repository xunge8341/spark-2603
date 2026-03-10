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
1) remove *double copy* (lease token -> borrowed slice fast-path with RAII release);
2) add an owned-segment append path (`Cumulation::push_segment(Bytes)`) to allow true zero-copy for large reads.

### Gap 2 — “可控分配”还缺证据闭环与硬上限（**current focus**）
**Now:** watermarks gate writability but do not hard-cap pending outbound bytes; queue growth can be app-driven.
Cumulation tail is pre-capacity hinted but may still reallocate up to max-frame.
**North Star:** hot-path allocations are either avoided or explicitly budgeted (hard cap) with observable counters.
**Next action:** define an allocation budget model (hard caps + counters), then stage:
- outbound hard cap option (default compatible);
- capacity-growth counters (VecDeque/BytesMut) and baseline reporting;
- later: bounded queues / pooling (only if evidence shows it matters).

### Gap 3 — ASP.NET Core experience needs config single-source-of-truth (**current focus**)
**Now:** Mgmt Profile v1 exists, but `ServerConfig` still mirrors mgmt fields and then re-derives the profile.
**North Star:** Options/Profile is the only user-facing source of truth; effective config is explainable and auditable.
**Next action:** collapse mgmt config into `MgmtTransportProfileV1` as the sole field in `ServerConfig`,
add `describe_effective_config()` for runtime audit, and freeze default sets.

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
