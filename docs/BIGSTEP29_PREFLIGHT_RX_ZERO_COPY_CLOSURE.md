# BigStep-29 Preflight — RX Zero-Copy Closure

## Status

BigStep-28 / Step-6 is treated as the frozen baseline. Driver scheduling kernel semantics, generation-aware correctness, task ownership, and perf evidence loops are already stabilized.

BigStep-29 must **not** start by rewriting the driver kernel or squeezing `ReadKick`. The correct first move is to audit the RX zero-copy closure boundary and add observability for the current trunk behavior.

## Current trunk shape

Today the effective RX lifetime is intentionally short:

`try_read_lease() -> rx_ptr_len(tok) -> materialize_rx_token() -> decode`

Or, on unsupported backends:

`try_read_lease() -> Unsupported -> try_read_into(read_buf) -> decode`

This means the system currently supports **lease-aware ingress**, but not end-to-end borrowed RX propagation through decoder, app futures, task scheduling, reclaim, or generation rotation.

## Phase-A goal

Phase-A adds stable counters for:

- lease hit count
- materialize count
- materialize bytes
- unsupported fallback count

This is intentionally narrow. It does **not** expand ownership lifetimes.

## Invariants

1. Borrowed RX must not outlive the channel reclaim boundary.
2. Borrowed RX must not cross app future yield/resume in Phase-A.
3. Driver kernel semantics are out of scope for this phase.
4. Unsupported backends must remain fully functional via copy fallback.

## Next candidate step after Phase-A

Only after counters are live and verified should trunk consider a same-stack borrowed stream-decode fast path. Even then, the borrowed view must remain same-stack and non-escaping.
