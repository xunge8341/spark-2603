# Benchmark artifacts

`scripts/perf_report.sh` generates machine-readable artifacts under `benchmark/reports/`:

- `perf_report.json`
- `perf_report.csv`

These files are intended for local/CI comparison and are not committed by default.

## Report contract

### `perf_report.json`

Top-level fields:

- `format_version`: report schema version.
- `scenarios[]`: one row per benchmark scenario.
- `global`: aggregate evidence used by perf gate.

Per-scenario metrics:

- `scenario`
- `throughput_rps`
- `p50_us`, `p95_us`, `p99_us`
- `syscalls_per_kib`
- `writev_share`
- `copy_per_byte`
- `backpressure_events`
- `peak_rss_kib`

Global metrics:

- `peak_inflight_buffer_bytes`
- `alloc_count`
- `alloc_bytes`
- `alloc_bytes_unavailable_reason`

`alloc_bytes` currently remains `null`: allocator-level byte accounting is not yet wired into
`perf_baseline`. `alloc_bytes_unavailable_reason` explains this explicitly to avoid a silent/ambiguous null.

### `perf_report.csv`

CSV rows mirror the scenario-level JSON fields and are intended for quick diffs in CI logs.

## Baseline files

Perf gate baselines live under `perf/baselines/`:

- `perf_gate_windows.json`: Windows-first thresholds.
- `perf_gate_unix.json`: Unix-first thresholds.
- `perf_gate_default.json`: fallback when platform-specific file is unavailable.

Resolution order in `perf_gate.{sh,ps1}`:

1. `SPARK_PERF_BASELINE` (explicit override)
2. platform baseline (`perf_gate_windows.json` / `perf_gate_unix.json`)
3. `perf_gate_default.json`

## Scenario definitions

Current perf matrix:

- `small_packet_high_freq`: tiny frames with high iteration count (syscall/batching sensitivity).
- `large_packet_streaming`: large frames for throughput + tail latency under sustained transfer.
- `slow_consumer`: stresses backpressure/queueing with reduced consume pace.
- `high_concurrency_short_conn`: many short-lived exchanges to exercise scheduling overhead.

## Where perf gate runs by default

- `scripts/verify.sh` / `scripts/verify.ps1`: **opt-in** (`SPARK_VERIFY_PERF_GATE=1`) to keep day-to-day checks fast and stable.
- `scripts/verify_nightly.sh` / `scripts/verify_nightly.ps1`: perf gate enabled by default.

## Local validation commands

### Bash

```bash
# Generate JSON/CSV reports.
bash ./scripts/perf_report.sh

# Run gate with baseline auto-resolution.
bash ./scripts/perf_gate.sh

# Verify missing-scenario failure (example using an intentionally incomplete baseline).
cat >/tmp/perf_gate_missing.json <<'JSON'
{"scenarios":[{"scenario":"missing_case","min":{"throughput_rps":1.0}}]}
JSON
SPARK_PERF_BASELINE=/tmp/perf_gate_missing.json bash ./scripts/perf_gate.sh
```

### PowerShell

```powershell
# Generate JSON/CSV reports.
bash ./scripts/perf_report.sh

# Run gate with baseline auto-resolution.
.\scripts\perf_gate.ps1

# Verify missing-scenario failure (example using an intentionally incomplete baseline).
$tmp = Join-Path $env:TEMP 'perf_gate_missing.json'
'{"scenarios":[{"scenario":"missing_case","min":{"throughput_rps":1.0}}]}' | Set-Content -Path $tmp
$env:SPARK_PERF_BASELINE = $tmp; .\scripts\perf_gate.ps1
```
