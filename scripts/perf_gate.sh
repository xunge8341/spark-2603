#!/usr/bin/env bash
set -euo pipefail

resolve_baseline_path() {
  if [[ -n "${SPARK_PERF_BASELINE:-}" ]]; then
    printf '%s\n' "$SPARK_PERF_BASELINE"
    return
  fi

  local platform_file="perf_gate_unix.json"
  if [[ "$(uname -s)" == *"NT"* ]] || [[ "$(uname -s)" == *"MINGW"* ]] || [[ "$(uname -s)" == *"MSYS"* ]]; then
    platform_file="perf_gate_windows.json"
  fi

  if [[ -f "perf/baselines/${platform_file}" ]]; then
    printf '%s\n' "perf/baselines/${platform_file}"
    return
  fi

  printf '%s\n' "perf/baselines/perf_gate_default.json"
}

BASELINE_PATH="$(resolve_baseline_path)"
REPORT_DIR="${SPARK_BENCH_REPORT_DIR:-benchmark/reports}"
REPORT_JSON="${SPARK_BENCH_REPORT_JSON:-$REPORT_DIR/perf_report.json}"
REPORT_CSV="${SPARK_BENCH_REPORT_CSV:-$REPORT_DIR/perf_report.csv}"

echo "Using perf baseline: $BASELINE_PATH"
echo "[1/2] generate benchmark report"
SPARK_BENCH_REPORT_DIR="$REPORT_DIR" SPARK_BENCH_REPORT_JSON="$REPORT_JSON" SPARK_BENCH_REPORT_CSV="$REPORT_CSV" \
  scripts/perf_report.sh

echo "[2/2] compare report with baseline"
python3 - "$BASELINE_PATH" "$REPORT_JSON" <<'PY'
import json
import sys

baseline_path, report_path = sys.argv[1], sys.argv[2]
with open(baseline_path, 'r', encoding='utf-8') as f:
    baseline = json.load(f)
with open(report_path, 'r', encoding='utf-8') as f:
    report = json.load(f)

scenarios = {s['scenario']: s for s in report.get('scenarios', [])}
errors = []

def check(name, field, actual, expected, mode):
    if mode == 'min' and actual < expected:
        errors.append(f"{name}: {field}={actual} < min {expected}")
    if mode == 'max' and actual > expected:
        errors.append(f"{name}: {field}={actual} > max {expected}")

for rule in baseline.get('scenarios', []):
    name = rule['scenario']
    actual = scenarios.get(name)
    if actual is None:
        errors.append(f"missing scenario in report: {name}")
        continue
    for field, threshold in rule.get('min', {}).items():
        check(name, field, actual.get(field, 0), threshold, 'min')
    for field, threshold in rule.get('max', {}).items():
        check(name, field, actual.get(field, 0), threshold, 'max')

global_rule = baseline.get('global', {})
global_actual = report.get('global', {})
for field, threshold in global_rule.get('max', {}).items():
    value = global_actual.get(field)
    if value is None:
        continue
    check('global', field, value, threshold, 'max')

if errors:
    print('Perf gate failed:')
    for err in errors:
        print(f' - {err}')
    sys.exit(1)

print('OK: perf gate passed for all scenarios.')
PY
