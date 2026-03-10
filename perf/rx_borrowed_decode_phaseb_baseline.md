# RX borrowed-decode Phase-B baseline (manual perf probe notes)

This note captures the intended reading of the `SPARK_RX_BORROWED_PERF` probe
added after BigStep-29 / Phase-B minimal landed.

## Probe role

`SPARK_RX_BORROWED_PERF` is not a throughput benchmark.
It is a deterministic evidence probe for the *leased stream + same-stack decode* path.

Healthy output should typically show:

- `leased >= 1`
- `borrowed_decode_attempt >= 1`
- `borrowed_decode_hit >= 1`
- `borrowed_decode_bytes > 0`
- `borrowed_decode_fallback = 0`
- `materialize = 0`
- `lease_fallback = 0`
- `released >= 1`

Interpretation:

- the backend exposed an RX lease;
- the fast path attempted borrowed decode;
- decode completed synchronously on the same stack;
- the token was released without first materializing into owned bytes.

## Practical operating rule

Do **not** hard-gate absolute hit counts yet.
Use this probe to confirm that future refactors do not silently regress the
leased stream fast path back into eager materialization.
