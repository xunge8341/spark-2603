#!/usr/bin/env bash
set -euo pipefail

die() {
  echo "ERROR: $*" 1>&2
  exit 1
}

if ! command -v python3 >/dev/null 2>&1; then
  die "perf gate requires python3. Install Python 3 and ensure 'python3' is on PATH, or run from a shell that provides it."
fi

REPORT_DIR="${SPARK_BENCH_REPORT_DIR:-benchmark/reports}"
REPORT_JSON="${SPARK_BENCH_REPORT_JSON:-$REPORT_DIR/perf_report.json}"
REPORT_CSV="${SPARK_BENCH_REPORT_CSV:-$REPORT_DIR/perf_report.csv}"

python3 ./scripts/perf_gate_core.py \
  --platform unix \
  --report-dir "$REPORT_DIR" \
  --report-json "$REPORT_JSON" \
  --report-csv "$REPORT_CSV" \
  --run-report-command -- ./scripts/perf_report.sh
