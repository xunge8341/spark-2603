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

Resolution order in `scripts/perf_gate_core.py`:

1. `SPARK_PERF_BASELINE` (explicit override)
2. platform baseline (`perf_gate_windows.json` / `perf_gate_unix.json`)
3. `perf_gate_default.json`

## Perf gate architecture (single core, thin wrappers)

- `scripts/perf_gate_core.py`: shared core for baseline resolution + JSON comparison.
- `scripts/perf_gate.sh`: Unix thin wrapper; dependency precheck + invoke core.
- `scripts/perf_gate.ps1`: Windows thin wrapper; dependency precheck + invoke core.

This reduces script drift: sh/ps1 no longer each maintain their own comparison implementation.

## Dependency matrix

| Entry | Linux/macOS | Windows PowerShell |
| --- | --- | --- |
| `scripts/perf_report.sh` | `bash`, `cargo` | `bash`, `cargo` |
| `scripts/perf_gate.sh` | `python3`, `bash`, `cargo` | n/a |
| `scripts/perf_gate.ps1` | n/a | `python3` (or `py -3` / `python`), `bash`, `cargo` |

Notes:

- Windows perf gate still calls `scripts/perf_report.sh` through `bash`; this is explicit and prechecked.
- Missing dependency failures are designed to be actionable (which binary is missing + how to install/provide it).

## Scenario definitions

Current perf matrix:

- `small_packet_high_freq`: tiny frames with high iteration count (syscall/batching sensitivity).
- `large_packet_streaming`: large frames for throughput + tail latency under sustained transfer.
- `slow_consumer`: stresses backpressure/queueing with reduced consume pace.
- `high_concurrency_short_conn`: many short-lived exchanges to exercise scheduling overhead.

## Verify vs nightly responsibility boundary

- `scripts/verify.sh` / `scripts/verify.ps1`: **perf gate opt-in** (`SPARK_VERIFY_PERF_GATE=1`) to keep PR/local checks deterministic and faster.
- `scripts/verify_nightly.sh` / `scripts/verify_nightly.ps1`: **perf gate default-on** to block regressions in the heavier nightly path.

This split keeps developer feedback loops short while still enforcing perf baselines on scheduled gates.

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
