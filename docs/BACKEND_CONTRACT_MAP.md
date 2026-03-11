# Backend Contract Map

This file is the source of truth for backend correctness scope and current status.

## P0 contracts and protected risks

| Contract | Risk prevented |
|---|---|
| close_evidence / abortive_close_evidence | zombie channels, shutdown inconsistency |
| flush_limited_progress | partial-writable stalls and tail-latency blowups |
| outbound_partial_vectored | loss/duplication under short write |
| framing_roundtrip (segments + random cuts) | protocol corruption under fragmentation |
| backpressure / drain / fairness | deadlock/starvation and scheduler collapse |

## Backend status baseline (2026-03)

| Backend crate | Current role | Contract status | Default gate status |
|---|---|---|---|
| `spark-transport-mio` | Primary dataplane backend | Primary path for contract execution | Blocking via `verify.sh` workspace + contract suite |
| `spark-transport-iocp` | Windows compatibility layer (phase-0 wrapper by default) | Runnable completion prototype exists behind feature flag; native socket overlapped path pending | Non-blocking unless `SPARK_VERIFY_COMPLETION_GATE=1` |
| `spark-engine-uring` | Local engine / bring-up scaffold | Not a parity backend yet | Not a dedicated blocking gate |

## Non-negotiable invariants

- Forward progress: queued outbound bytes must keep getting bounded flush attempts.
- No semantic leakage: edge/level/completion differences cannot leak to user semantics.
- Stable observability: evidence/metric names are frozen in `spark_uci::names`.
- Driver kernel contract baseline stays frozen: `install_channel() -> sync_interest(chan_id)`.

## Bring-up order (still valid)

1. Freeze semantics in core driver/contract tests.
2. Close Windows IOCP correctness gap.
3. Keep Linux/macOS parity expansion mechanical (epoll/kqueue/io_uring) via same contracts.
