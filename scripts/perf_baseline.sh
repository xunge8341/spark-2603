#!/usr/bin/env bash
set -euo pipefail

echo "[1/2] cargo test -p spark-transport-contract --test perf_baseline --no-run"
cargo test -p spark-transport-contract --test perf_baseline --no-run

echo "[2/2] cargo test -p spark-transport-contract --test perf_baseline -- --ignored --nocapture"
cargo test -p spark-transport-contract --test perf_baseline -- --ignored --nocapture

echo "Note: SPARK_PERF line now includes alloc evidence (outbound/cumulation growth + peak)."
echo "OK: perf baseline completed."
