# ADR-012: Runnable gate for IOCP completion prototype

## Status
Accepted (BigStep-18)

## Context
We have introduced a **completion-style** reactor foundation in `spark-transport` and a Windows-native IOCP
completion port polling prototype in `spark-transport-iocp` (feature-gated as `native-completion`).

A compile-only gate can still allow:
- runtime failures when creating the completion port handle,
- silent ABI/signature drift across Windows SDK / `windows-sys` versions,
- accidental regressions that only appear when calling `GetQueuedCompletionStatusEx`.

At the same time, we explicitly do **not** want to pull the high-risk surface of overlapped submission
(AcceptEx/WSARecv/WSASend, cancellation, close semantics) into the trunk dataplane yet.

## Decision
Upgrade the completion prototype gate from **compile-only** to **runnable**:

- Add a deterministic, fast smoke test:
  - create IOCP completion port,
  - poll with 0-timeout budget,
  - assert success and zero events.
- When `SPARK_VERIFY_COMPLETION_GATE=1`:
  - run `cargo test -p spark-transport-iocp --features native-completion`.

## Consequences
- The completion prototype can iterate without bit-rot, with minimal risk to trunk.
- Submission APIs remain out of scope and will be introduced with dedicated contract suites.

## Notes (future work)
- Add submission APIs behind an additional feature (e.g. `native-submit`).
- Add P0 contracts for:
  - cancellation/close ordering,
  - partial progress semantics,
  - backpressure/flush fairness under completion delivery.
