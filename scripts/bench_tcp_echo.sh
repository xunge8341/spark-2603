#!/usr/bin/env bash
set -euo pipefail

# Run the mio TCP echo micro-bench (ignored test) and print a machine-readable summary line.
#
# Environment variables:
# - SPARK_BENCH_CONCURRENCY (default: 32)
# - SPARK_BENCH_REQS_PER_CONN (default: 256)

echo "[1/2] cargo test -p spark-transport-mio --test bench_tcp_echo --no-run"
cargo test -p spark-transport-mio --test bench_tcp_echo --no-run

echo "[2/2] cargo test -p spark-transport-mio --test bench_tcp_echo -- --ignored --nocapture"
OUTPUT="$(cargo test -p spark-transport-mio --test bench_tcp_echo -- --ignored --nocapture 2>&1)"
printf '%s\n' "$OUTPUT"

BENCH_LINE="$(printf '%s\n' "$OUTPUT" | grep '^[[:space:]]*SPARK_BENCH ' | tail -n 1 || true)"
if [[ -z "$BENCH_LINE" ]]; then
  echo "Failed to find SPARK_BENCH output line." >&2
  exit 1
fi

echo "OK: bench completed."
