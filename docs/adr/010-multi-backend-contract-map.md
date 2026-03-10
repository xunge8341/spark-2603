# ADR-010: Multi-backend bring-up via contract mapping

## Status
Accepted

## Context
Spark's North Star requires a cross-OS/CPU communication substrate with consistent semantics across:
Windows (IOCP), Linux (epoll/io_uring), macOS (kqueue), and future targets (WASI/WASM, embedded).

The dominant failure mode in multi-backend systems is **semantic drift**: each backend "mostly works"
but differs subtly under load (partial writes, close races, missing readiness events), causing p99/p999 instability.

## Decision
We will treat backend bring-up as a **contract-driven port**:

- Define backend correctness in a **P0 contract map**.
- Require every backend to pass the same contract suite.
- Absorb backend event-model differences in the driver/adapter layer.
- Keep observability names/fields frozen in `spark_uci::names` as part of the contract.

## Consequences
- Backend work becomes predictable and reviewable (contract failures are actionable).
- We avoid long-lived compatibility shims (no intermediate user semantics).
- Performance tuning is deferred until correctness is contract-locked.
