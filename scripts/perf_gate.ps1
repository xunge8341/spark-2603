#!/usr/bin/env pwsh
<#!
Run perf report + JSON baseline comparison on Windows/PowerShell.

Baseline resolution order:
1) SPARK_PERF_BASELINE
2) perf/baselines/perf_gate_windows.json (on Windows)
3) perf/baselines/perf_gate_default.json
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Resolve-BaselinePath {
  $explicit = [Environment]::GetEnvironmentVariable("SPARK_PERF_BASELINE")
  if (-not [string]::IsNullOrWhiteSpace($explicit)) {
    return $explicit
  }

  $platformFile = if ($env:OS -eq "Windows_NT") { "perf_gate_windows.json" } else { "perf_gate_unix.json" }
  $platformPath = Join-Path "perf/baselines" $platformFile
  if (Test-Path $platformPath) {
    return $platformPath
  }

  return "perf/baselines/perf_gate_default.json"
}

$BaselinePath = Resolve-BaselinePath
$ReportDir = if ([string]::IsNullOrWhiteSpace($env:SPARK_BENCH_REPORT_DIR)) { "benchmark/reports" } else { $env:SPARK_BENCH_REPORT_DIR }
$ReportJson = if ([string]::IsNullOrWhiteSpace($env:SPARK_BENCH_REPORT_JSON)) { Join-Path $ReportDir "perf_report.json" } else { $env:SPARK_BENCH_REPORT_JSON }
$ReportCsv = if ([string]::IsNullOrWhiteSpace($env:SPARK_BENCH_REPORT_CSV)) { Join-Path $ReportDir "perf_report.csv" } else { $env:SPARK_BENCH_REPORT_CSV }

Write-Host "Using perf baseline: $BaselinePath" -ForegroundColor DarkGray
Write-Host "[1/2] generate benchmark report" -ForegroundColor Cyan
$env:SPARK_BENCH_REPORT_DIR = $ReportDir
$env:SPARK_BENCH_REPORT_JSON = $ReportJson
$env:SPARK_BENCH_REPORT_CSV = $ReportCsv
& bash ./scripts/perf_report.sh
if ($LASTEXITCODE -ne 0) {
  throw "Step failed ([1/2] generate benchmark report) with exit code $LASTEXITCODE"
}

Write-Host "[2/2] compare report with baseline" -ForegroundColor Cyan
$pythonScript = @'
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
'@

& python3 -c $pythonScript -- $BaselinePath $ReportJson
if ($LASTEXITCODE -ne 0) {
  throw "Step failed ([2/2] compare report with baseline) with exit code $LASTEXITCODE"
}
