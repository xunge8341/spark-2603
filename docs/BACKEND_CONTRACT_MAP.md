# Backend Contract Map

This document is the **single source of truth** for what "backend correctness" means for Spark transport.

The goal is to make multi-backend bring-up (IOCP / epoll / kqueue / io_uring / WASI) **mechanical**:
each backend must pass the same P0 contract suite, and backend-specific event-model differences must be
**absorbed in the driver**, never leaked into user semantics.

## P0 contracts and what they protect

| Contract | Risk it prevents | Backend differences absorbed |
|---|---|---|
| close_evidence / abortive_close_evidence | zombie channels, inconsistent shutdown | completion vs readiness close notification; RST/ERROR mapping |
| flush_limited_progress | tail-latency blow-ups under partial writability | writable event loss; edge vs level readiness |
| outbound_partial_vectored | data loss/duplication under partial writes | short write semantics; iovec partial progress |
| framing_roundtrip (segments + random cuts) | protocol corruption under segmentation | recv fragmentation differences |
| backpressure / drain / fairness | deadlocks and starvation | event batching, scheduling |

## Invariants every backend must preserve

- **Forward progress**: once bytes are queued, bounded driver ticks must eventually attempt flush.
- **No semantic leakage**: edge/level/completion differences must not change observable behavior.
- **Stable observability**: evidence + metrics names/fields are frozen in `spark_uci::names`.

## Bring-up order (North Star aligned)

1. **Freeze surface** (traits + ADR), no behavior changes.
2. **IOCP correctness** first (Windows is a hard requirement).
3. **epoll** (Linux) then **kqueue** (macOS).
4. Add **io_uring** and **WASI sockets** as opt-in backends behind feature flags.

