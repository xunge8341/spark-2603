#!/usr/bin/env bash
set -euo pipefail

# Run the local dataplane perf smoke and fail if coarse baseline thresholds regress.
# Threshold resolution order:
# 1) Baseline file (SPARK_PERF_BASELINE, else platform default under perf/baselines/)
# 2) Explicit env var overrides:
#    - SPARK_MAX_SYSCALLS_PER_KIB
#    - SPARK_MIN_WRITEV_SHARE

resolve_baseline_path() {
  if [[ -n "${SPARK_PERF_BASELINE:-}" ]]; then
    printf '%s\n' "$SPARK_PERF_BASELINE"
    return
  fi

  if [[ -f "perf/baselines/unix.toml" ]]; then
    printf '%s\n' "perf/baselines/unix.toml"
    return
  fi

  printf '%s\n' "perf/baselines/default.toml"
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
BASELINE_MAX_SYSCALLS_PER_KIB="$(get_baseline_value "$BASELINE_PATH" "max_syscalls_per_kib" "1.0")"
BASELINE_MIN_WRITEV_SHARE="$(get_baseline_value "$BASELINE_PATH" "min_writev_share" "0.5")"
BASELINE_PROFILE="$(get_baseline_value "$BASELINE_PATH" "profile" "default")"

MAX_SYSCALLS_PER_KIB="${SPARK_MAX_SYSCALLS_PER_KIB:-$BASELINE_MAX_SYSCALLS_PER_KIB}"
MIN_WRITEV_SHARE="${SPARK_MIN_WRITEV_SHARE:-$BASELINE_MIN_WRITEV_SHARE}"

echo "Using perf baseline: $BASELINE_PATH (profile=$BASELINE_PROFILE, max_syscalls_per_kib=$MAX_SYSCALLS_PER_KIB, min_writev_share=$MIN_WRITEV_SHARE)"

echo "[1/2] cargo test -p spark-transport-contract --test perf_baseline --no-run --locked"
cargo test -p spark-transport-contract --test perf_baseline --no-run --locked

echo "[2/2] cargo test -p spark-transport-contract --test perf_baseline -- --ignored --nocapture"
OUTPUT="$(cargo test -p spark-transport-contract --locked --test perf_baseline -- --ignored --nocapture 2>&1)"
printf '%s\n' "$OUTPUT"

PERF_LINE="$(printf '%s\n' "$OUTPUT" | grep '^[[:space:]]*SPARK_PERF ' | tail -n 1 || true)"
if [[ -z "$PERF_LINE" ]]; then
  echo "Failed to find SPARK_PERF output line." >&2
  exit 1
fi

SYSCALLS_PER_KIB="$(printf '%s\n' "$PERF_LINE" | sed -n 's/.*syscalls_per_kib=\([0-9.][0-9.]*\).*/\1/p')"
WRITEV_SHARE="$(printf '%s\n' "$PERF_LINE" | sed -n 's/.*writev_share=\([0-9.][0-9.]*\).*/\1/p')"
if [[ -z "$SYSCALLS_PER_KIB" || -z "$WRITEV_SHARE" ]]; then
  echo "Failed to parse perf metrics from: $PERF_LINE" >&2
  exit 1
fi

awk -v actual="$SYSCALLS_PER_KIB" -v max="$MAX_SYSCALLS_PER_KIB" 'BEGIN { if (actual > max) exit 1 }' || {
  echo "Perf gate failed: syscalls_per_kib=$SYSCALLS_PER_KIB exceeds $MAX_SYSCALLS_PER_KIB" >&2
  exit 1
}

awk -v actual="$WRITEV_SHARE" -v min="$MIN_WRITEV_SHARE" 'BEGIN { if (actual < min) exit 1 }' || {
  echo "Perf gate failed: writev_share=$WRITEV_SHARE is below $MIN_WRITEV_SHARE" >&2
  exit 1
}

echo "OK: perf gate passed (profile=$BASELINE_PROFILE, syscalls_per_kib=$SYSCALLS_PER_KIB, writev_share=$WRITEV_SHARE)."
