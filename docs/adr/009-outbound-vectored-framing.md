# ADR-009: Outbound Vectored Framing (Zero-Copy TX)

- Status: Accepted
- Date: 2026-03-03

## Context

The North Star requires C++-class performance on the TX hot path:

- controlled or zero allocations on the hot path;
- prefer zero-copy payload handling;
- maximize vectored syscalls (`writev` / `WSASend`) and reduce syscall-per-byte;
- preserve p99/p999 stability under write pressure.

The repository already has allocation-free encoder primitives in `spark-codec`:

- `LineEncoder` / `DelimiterEncoder` (idempotent append)
- `LengthFieldPrepender`
- `ProtobufVarint32LengthFieldPrepender`

However, the transport pipeline previously materialized encoded output as a single `Bytes` by allocating and copying payloads. This defeats the purpose of both zero-copy payloads and vectored writes.

## Decision

Introduce a small, allocation-free vectored outbound representation and carry it through the pipeline and outbound buffer:

1) **`OutboundFrame`**
   - up to 3 segments (prefix / payload / suffix);
   - prefix and delimiters use an inline buffer (≤ 16 bytes) to avoid heap allocation;
   - payload remains `Bytes` (Arc-backed, sliceable) for zero-copy.

2) **`OutboundBuffer`**
   - queue `VecDeque<OutboundFrame>` instead of `VecDeque<Bytes>`;
   - flush builds a stack-only iovec across **frames + segments**, bounded by `MAX_IOV` and fairness budget;
   - partial-write progress is tracked across segment boundaries.

3) **Pipeline outbound type**
   - outbound write events carry `OutboundFrame`;
   - the head handler enqueues the vectored frame directly;
   - `StreamFrameEncoderHandler` becomes fully allocation-free:
     - Line/Delimiter append an inline suffix (idempotent);
     - LengthField/Varint32 prepend an inline header.

## Consequences

### Positive

- TX framing becomes symmetric and zero-copy for payloads.
- Vectored writes are now meaningful: the encoder no longer collapses frames into a single contiguous buffer.
- The representation is small, predictable, and easy to audit (no dynamic segment vectors).

### Negative / Trade-offs

- Slightly more complex flush logic (segment-aware cursor).
- The maximum segment count is bounded (3). This is deliberate; future protocols that need more can introduce a dedicated protocol stack layer (L4) rather than bloating the transport core.

## Validation

- Existing contract tests (`framing_roundtrip*`) must continue to pass.
- Performance baseline scripts (`perf_gate`, `perf_baseline`) must not regress and should show improved `writev_share_ratio` for delimiter/length-field workloads.
