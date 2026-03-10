#!/usr/bin/env bash
set -euo pipefail

# Run the mio TCP echo micro-bench and fail if coarse baseline thresholds regress.
#
# Threshold resolution order:
# 1) Baseline file (SPARK_BENCH_BASELINE, else platform default under perf/baselines/)
# 2) Explicit env var overrides:
#    - SPARK_BENCH_MIN_RPS
#    - SPARK_BENCH_MAX_P99_US

resolve_baseline_path() {
  if [[ -n "${SPARK_BENCH_BASELINE:-}" ]]; then
    printf '%s\n' "$SPARK_BENCH_BASELINE"
    return
  fi

  local platform_file="bench_tcp_echo_unix.toml"
  if [[ "$(uname -s)" == *"NT"* ]] || [[ "$(uname -s)" == *"MINGW"* ]] || [[ "$(uname -s)" == *"MSYS"* ]]; then
    platform_file="bench_tcp_echo_windows.toml"
  fi

  if [[ -f "perf/baselines/${platform_file}" ]]; then
    printf '%s\n' "perf/baselines/${platform_file}"
    return
  fi

  printf '%s\n' "perf/baselines/bench_tcp_echo_default.toml"
}

get_baseline_value() {
  local path="$1"
  local key="$2"
  local default_value="$3"

  if [[ ! -f "$path" ]]; then
    printf '%s\n' "$default_value"
    return
  fi

  local raw
  raw="$(sed -n "s/^[[:space:]]*${key}[[:space:]]*=[[:space:]]*//p" "$path" | head -n 1 || true)"
  if [[ -z "$raw" ]]; then
    printf '%s\n' "$default_value"
    return
  fi

  raw="${raw%\"}"
  raw="${raw#\"}"
  printf '%s\n' "$raw"
}

BASELINE_PATH="$(resolve_baseline_path)"
BASELINE_MIN_RPS="$(get_baseline_value "$BASELINE_PATH" "min_rps" "1000")"
BASELINE_MAX_P99_US="$(get_baseline_value "$BASELINE_PATH" "max_p99_us" "50000")"
BASELINE_PROFILE="$(get_baseline_value "$BASELINE_PATH" "profile" "default")"

MIN_RPS="${SPARK_BENCH_MIN_RPS:-$BASELINE_MIN_RPS}"
MAX_P99_US="${SPARK_BENCH_MAX_P99_US:-$BASELINE_MAX_P99_US}"

echo "Using bench baseline: $BASELINE_PATH (profile=$BASELINE_PROFILE, min_rps=$MIN_RPS, max_p99_us=$MAX_P99_US)"

echo "[1/2] cargo test -p spark-transport-mio --test bench_tcp_echo --no-run --locked"
cargo test -p spark-transport-mio --test bench_tcp_echo --no-run --locked

echo "[2/2] cargo test -p spark-transport-mio --test bench_tcp_echo -- --ignored --nocapture"
OUTPUT="$(cargo test -p spark-transport-mio --locked --test bench_tcp_echo -- --ignored --nocapture 2>&1)"
printf '%s\n' "$OUTPUT"

BENCH_LINE="$(printf '%s\n' "$OUTPUT" | grep '^[[:space:]]*SPARK_BENCH ' | tail -n 1 || true)"
if [[ -z "$BENCH_LINE" ]]; then
  echo "Failed to find SPARK_BENCH output line." >&2
  exit 1
fi

RPS="$(printf '%s\n' "$BENCH_LINE" | sed -n 's/.*rps=\([0-9.][0-9.]*\).*/\1/p')"
P99_US="$(printf '%s\n' "$BENCH_LINE" | sed -n 's/.*p99_us=\([0-9][0-9]*\).*/\1/p')"
if [[ -z "$RPS" || -z "$P99_US" ]]; then
  echo "Failed to parse bench metrics from: $BENCH_LINE" >&2
  exit 1
fi

awk -v actual="$RPS" -v min="$MIN_RPS" 'BEGIN { if (actual < min) exit 1 }' || {
  echo "Bench gate failed: rps=$RPS is below $MIN_RPS" >&2
  exit 1
}

awk -v actual="$P99_US" -v max="$MAX_P99_US" 'BEGIN { if (actual > max) exit 1 }' || {
  echo "Bench gate failed: p99_us=$P99_US exceeds $MAX_P99_US" >&2
  exit 1
}

echo "OK: bench gate passed (profile=$BASELINE_PROFILE, rps=$RPS, p99_us=$P99_US)."
