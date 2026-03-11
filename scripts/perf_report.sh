#!/usr/bin/env bash
set -euo pipefail

OUT_DIR="${SPARK_BENCH_REPORT_DIR:-benchmark/reports}"
OUT_JSON="${SPARK_BENCH_REPORT_JSON:-$OUT_DIR/perf_report.json}"
OUT_CSV="${SPARK_BENCH_REPORT_CSV:-$OUT_DIR/perf_report.csv}"
mkdir -p "$OUT_DIR"

extract_metric() {
  local line="$1"
  local key="$2"
  printf '%s\n' "$line" | sed -n "s/.*${key}=\([^ ]*\).*/\1/p"
}

run_perf_case() {
  local scenario="$1"
  local frames="$2"
  local frame_bytes="$3"
  local iterations="$4"
  local allowance_divisor="$5"

  local cmd=(cargo test -p spark-transport-contract --locked --test perf_baseline -- --ignored --nocapture)
  local output
  if [[ -x /usr/bin/time ]]; then
    output="$(SPARK_PERF_SCENARIO="$scenario" SPARK_PERF_FRAMES="$frames" SPARK_PERF_FRAME_BYTES="$frame_bytes" SPARK_PERF_ITERATIONS="$iterations" SPARK_PERF_ALLOWANCE_DIVISOR="$allowance_divisor" /usr/bin/time -f 'SPARK_RSS max_rss_kib=%M' "${cmd[@]}" 2>&1)"
  else
    output="$(SPARK_PERF_SCENARIO="$scenario" SPARK_PERF_FRAMES="$frames" SPARK_PERF_FRAME_BYTES="$frame_bytes" SPARK_PERF_ITERATIONS="$iterations" SPARK_PERF_ALLOWANCE_DIVISOR="$allowance_divisor" "${cmd[@]}" 2>&1)"
  fi
  printf '%s\n' "$output"

  local perf_line rss_line
  perf_line="$(printf '%s\n' "$output" | grep '^[[:space:]]*SPARK_PERF ' | tail -n 1 || true)"
  rss_line="$(printf '%s\n' "$output" | grep '^[[:space:]]*SPARK_RSS ' | tail -n 1 || true)"
  if [[ -z "$perf_line" ]]; then
    echo "Missing SPARK_PERF line for scenario=$scenario" >&2
    exit 1
  fi

  local throughput p50 p95 p99 spk ws copy bp peak_i alloc_c rss_kib
  throughput="$(extract_metric "$perf_line" "throughput_bytes_per_sec")"
  p50="$(extract_metric "$perf_line" "p50_us")"
  p95="$(extract_metric "$perf_line" "p95_us")"
  p99="$(extract_metric "$perf_line" "p99_us")"
  spk="$(extract_metric "$perf_line" "syscalls_per_kib")"
  ws="$(extract_metric "$perf_line" "writev_share")"
  copy="$(extract_metric "$perf_line" "copy_per_byte")"
  bp="$(extract_metric "$perf_line" "backpressure_events")"
  peak_i="$(extract_metric "$perf_line" "ob_peak_pending_bytes")"
  alloc_c="$(extract_metric "$perf_line" "ob_q_growth")"

  rss_kib="-1"
  if [[ -n "$rss_line" ]]; then
    parsed_rss="$(extract_metric "$rss_line" "max_rss_kib")"
    if [[ -n "$parsed_rss" ]]; then
      rss_kib="$parsed_rss"
    fi
  fi

  printf '%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s\n' \
    "$scenario" "$throughput" "$p50" "$p95" "$p99" "$spk" "$ws" "$copy" "$bp" "$rss_kib" "$peak_i" >> "$OUT_CSV"

  printf '%s|%s|%s|%s|%s|%s|%s|%s|%s|%s|%s|%s\n' \
    "$scenario" "$throughput" "$p50" "$p95" "$p99" "$spk" "$ws" "$copy" "$bp" "$rss_kib" "$peak_i" "$alloc_c"
}

echo "scenario,throughput_bytes_per_sec,p50_us,p95_us,p99_us,syscalls_per_kib,writev_share,copy_per_byte,backpressure_events,peak_rss_kib,peak_inflight_buffer_bytes" > "$OUT_CSV"

cases=(
  "small_packet_high_freq 2048 64 96 1"
  "large_packet_streaming 256 16384 48 1"
  "slow_consumer 1024 512 64 8"
  "high_concurrency_short_conn 512 256 128 1"
)

rows=()
alloc_max=0
peak_inflight_max=0
for c in "${cases[@]}"; do
  # shellcheck disable=SC2086
  row="$(run_perf_case $c | tail -n 1)"
  rows+=("$row")
  row_peak="$(printf '%s' "$row" | cut -d '|' -f 11)"
  row_alloc="$(printf '%s' "$row" | cut -d '|' -f 12)"
  if (( row_peak > peak_inflight_max )); then
    peak_inflight_max="$row_peak"
  fi
  if (( row_alloc > alloc_max )); then
    alloc_max="$row_alloc"
  fi
done

python3 - "$OUT_JSON" "$peak_inflight_max" "$alloc_max" "${rows[@]}" <<'PY'
import json
import sys
out_json = sys.argv[1]
peak_inflight = int(sys.argv[2])
alloc_count = int(sys.argv[3])
rows = sys.argv[4:]

items = []
for row in rows:
    parts = row.split('|')
    items.append({
        "scenario": parts[0],
        "throughput_rps": float(parts[1]),
        "p50_us": int(parts[2]),
        "p95_us": int(parts[3]),
        "p99_us": int(parts[4]),
        "syscalls_per_kib": float(parts[5]),
        "writev_share": float(parts[6]),
        "copy_per_byte": float(parts[7]),
        "backpressure_events": int(parts[8]),
        "peak_rss_kib": int(parts[9]),
    })

report = {
    "format_version": 1,
    "scenarios": items,
    "global": {
        "peak_inflight_buffer_bytes": peak_inflight,
        "alloc_count": alloc_count,
        "alloc_bytes": None,
        "alloc_bytes_unavailable_reason": "allocator-level byte accounting is not wired into perf_baseline yet; report keeps a stable placeholder instead of fabricating bytes",
    },
}
with open(out_json, 'w', encoding='utf-8') as f:
    json.dump(report, f, indent=2, sort_keys=True)
print(json.dumps(report, indent=2, sort_keys=True))
PY

echo "Wrote $OUT_JSON"
echo "Wrote $OUT_CSV"
