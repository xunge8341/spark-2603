# Performance baselines

This directory stores versioned perf-gate thresholds used by local scripts and CI.

## Production-style perf gate

Use `scripts/perf_gate.sh`.

It now runs a scenario matrix and validates report-vs-baseline instead of parsing a single line:

1. Generates a machine-readable report via `scripts/perf_report.sh`:
   - JSON: `benchmark/reports/perf_report.json`
   - CSV: `benchmark/reports/perf_report.csv`
2. Compares the report against per-platform baselines:
   - `perf/baselines/perf_gate_unix.json`
   - `perf/baselines/perf_gate_windows.json`
   - `perf/baselines/perf_gate_default.json`

Baseline resolution order:

1. `SPARK_PERF_BASELINE`
2. Platform default (`perf_gate_windows.json` on Windows, `perf_gate_unix.json` on Unix)
3. `perf_gate_default.json`

## Scenario matrix (minimum)

The report includes at least these trunk scenarios:

- `small_packet_high_freq`
- `large_packet_streaming`
- `slow_consumer`
- `high_concurrency_short_conn`

Each scenario tracks:

- latency (`p50_us`, `p95_us`, `p99_us`)
- throughput (`throughput_rps`)
- syscall & batching efficiency (`syscalls_per_kib`, `writev_share`)
- copy pressure (`copy_per_byte`)
- backpressure trigger count (`backpressure_events`)
- peak RSS (best-effort `peak_rss_kib`; `-1` if unsupported by host toolchain)

Global report fields:

- `peak_inflight_buffer_bytes`
- `alloc_count`
- `alloc_bytes` (`null` for now; optional allocator instrumentation)

## Platform honesty

Baselines are split by platform on purpose.

- Unix and Windows thresholds are both first-class.
- We do **not** reuse Linux-only “best looking” numbers as universal defaults.
- `platform_note` in each baseline must explain the expectation level.
