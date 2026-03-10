# Driver kernel Step-6 baseline (manual perf probe notes)

This note captures the expected interpretation of the three manual perf-baseline probe lines introduced in BigStep-28 / Step-6.
It is intentionally descriptive, not a hard gate.

## Probe roles

### `SPARK_DRIVER_PERF`
Generic summary probe. Healthy output typically looks like:

- `schedule_write_kick = 0`
- `schedule_flush_followup > 0`
- `schedule_read_kick >= 0`
- `task_submit == task_finish`
- `task_state_conflict = 0`

Interpretation: the generic inline-flush sample is being advanced by explicit `FlushFollowup`, not by a redundant inline `WriteKick`.

### `SPARK_DRIVER_PERF_WRITEKICK`
WRITE-edge bootstrap probe. Expected invariants:

- `schedule_write_kick >= 1`
- `task_submit == task_finish`
- `task_state_conflict = 0`
- `tx_bytes` covers the large-response payload

Interpretation: when WRITE interest rises under backpressure, the driver still manufactures the one bootstrap kick required by edge-triggered backends.

### `SPARK_DRIVER_PERF_READKICK`
Read-resume probe. Expected invariants:

- `app_calls = 2`
- `schedule_read_kick >= 1`
- `task_submit == task_finish`
- `task_state_conflict = 0`

Current local samples on the synthetic backpressure scenario show:

- `schedule_read_kick = 2`
- `task_submit = 3`
- `app_calls = 2`

This is currently treated as acceptable. It proves that reads resume without a second reactor `Readable` edge, and there is not yet enough evidence to safely coalesce read-side work further.

## Practical operating rule

Do not tighten perf gates around these counters yet.
Use them as evidence for future changes.
A future optimization that tries to reduce `ReadKick` further should first add a stronger probe that can distinguish:

1. a required read-resume bootstrap after backpressure exit, from
2. a truly empty follow-up.


### `SPARK_RX_BORROWED_PERF`
RX borrowed-decode probe for the BigStep-29 / Phase-B minimal fast path.

Expected invariants on the synthetic leased-stream scenario:

- `leased >= 1`
- `borrowed_decode_attempt >= 1`
- `borrowed_decode_hit >= 1`
- `materialize = 0`
- `lease_fallback = 0`
- `released >= 1`

Interpretation: token-backed stream ingress is staying on the same stack long
enough to decode without materializing owned bytes, while still releasing the
lease through the single structured return path.
