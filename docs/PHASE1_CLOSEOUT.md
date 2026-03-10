# Phase 1 Closeout (Transport Core GA + Symmetric Framing)

This document is the **closeout** for Phase 1. It captures what is *done*, what is intentionally
*deferred*, and what remains to reach the North Star.

## What is considered DONE in Phase 1 (trunk)

### Contract-first transport core
- P0 contract suite covers: close/half-close, flush fairness/limited progress, partial write ordering, error classification.
- Default library semantics are panic-free (gate via `scripts/panic_scan.*`).

### Observability is a contract
- Evidence and metric names are centralized in `spark-uci::names` (no_std anchor).
- Mgmt `/healthz` and `/metrics` are dogfooding-gated; metrics names are hard-bound to the constants.

### Symmetric framing (RX + TX)
- Inbound framing primitives are supported.
- Outbound is symmetric and uses vectored frames (`OutboundFrame`) to preserve zero-copy payloads and enable writev batching.
- Partial vectored progress is contract-tested.

### Multi-backend bring-up scaffolding
- Backend contract map exists (`docs/BACKEND_CONTRACT_MAP.md`).
- Windows distribution path exists (`spark-dist-iocp`) with mgmt + dataplane echo smokes.
- Completion foundation exists (feature-gated) and is runnable-gated on Windows (`SPARK_VERIFY_COMPLETION_GATE=1`), including a minimal submit → completion → poll closure via posted completion packets.

## Deferred (explicitly non-blocking)
- Windows/mio `write_pressure_smoke` remains tracked in `docs/KNOWN_ISSUES.md` and is ignored on Windows.
  This is a platform-specific edge case and does not block trunk progression.

## Remaining work to reach North Star (next milestones)

### Milestone 4: Multi-backend parity
- Implement native IOCP completion submission (AcceptEx/WSARecv/WSASend) and pass P0 contracts.
- Bring up Linux epoll and macOS kqueue backends with the same contract suite.

### Milestone 5: Performance + Safety hardening
- CI/Nightly perf regression gates (p99/p999, syscalls/KB, copy/byte, memory).
- Fuzzing for framing + state machine; deterministic schedule tests for concurrency semantics.

See `docs/GAP_STATUS.md` for the always-current prioritized gap list.
