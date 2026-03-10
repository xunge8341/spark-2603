# Performance baselines

This directory stores coarse, versioned perf gate thresholds used by local scripts and CI.

The `scripts/perf_gate.*` helpers resolve a baseline file in this order:

1. `SPARK_PERF_BASELINE` explicit path
2. Platform default (`perf/baselines/windows.toml` on Windows, `perf/baselines/unix.toml` on Unix)
3. `perf/baselines/default.toml` fallback

Environment variables still win over file values:

- `SPARK_MAX_SYSCALLS_PER_KIB`
- `SPARK_MIN_WRITEV_SHARE`

This keeps the gate reproducible while still allowing emergency local overrides.

## Mio cross-platform micro-bench

The mio backend is used as the *cross-platform gate* (Windows/Linux/macOS) in M4.

For a coarse latency/throughput sanity check, run the TCP echo micro-bench:

- PowerShell: `scripts/bench_tcp_echo.ps1`
- Bash: `scripts/bench_tcp_echo.sh`

The bench gate (`scripts/bench_gate.*`) resolves its baseline in this order:

1. `SPARK_BENCH_BASELINE` explicit path
2. Platform default (`perf/baselines/bench_tcp_echo_windows.toml` on Windows, `perf/baselines/bench_tcp_echo_unix.toml` on Unix)
3. `perf/baselines/bench_tcp_echo_default.toml` fallback

Environment overrides:

- `SPARK_BENCH_MIN_RPS`
- `SPARK_BENCH_MAX_P99_US`

The bench itself prints a single `SPARK_BENCH ...` line, which both scripts parse.
