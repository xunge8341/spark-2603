#!/usr/bin/env pwsh
<#
Run perf report + baseline comparison via a shared Python core.

Windows dependencies for perf gate:
- bash (for scripts/perf_report.sh)
- Python 3 (python3, or py -3, or python)
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Fail-Dependency {
  param([string]$Message)
  throw "Dependency check failed: $Message"
}

function Resolve-PythonCommand {
  if (Get-Command python3 -ErrorAction SilentlyContinue) {
    return @("python3")
  }

  if (Get-Command py -ErrorAction SilentlyContinue) {
    return @("py", "-3")
  }

  if (Get-Command python -ErrorAction SilentlyContinue) {
    return @("python")
  }

  Fail-Dependency "Python 3 not found. Install Python 3 and ensure one of [python3, py -3, python] is available on PATH."
}

if (-not (Get-Command bash -ErrorAction SilentlyContinue)) {
  Fail-Dependency "bash not found. perf report currently runs via scripts/perf_report.sh; install Git Bash/WSL/MSYS2 and ensure 'bash' is on PATH."
}

$pythonCmd = Resolve-PythonCommand
$ReportDir = if ([string]::IsNullOrWhiteSpace($env:SPARK_BENCH_REPORT_DIR)) { "benchmark/reports" } else { $env:SPARK_BENCH_REPORT_DIR }
$ReportJson = if ([string]::IsNullOrWhiteSpace($env:SPARK_BENCH_REPORT_JSON)) { Join-Path $ReportDir "perf_report.json" } else { $env:SPARK_BENCH_REPORT_JSON }
$ReportCsv = if ([string]::IsNullOrWhiteSpace($env:SPARK_BENCH_REPORT_CSV)) { Join-Path $ReportDir "perf_report.csv" } else { $env:SPARK_BENCH_REPORT_CSV }

$cmd = @(
  "./scripts/perf_gate_core.py",
  "--platform", "windows",
  "--report-dir", $ReportDir,
  "--report-json", $ReportJson,
  "--report-csv", $ReportCsv,
  "--run-report-command", "--", "bash", "./scripts/perf_report.sh"
)

if ($pythonCmd.Length -gt 1) {
  & $pythonCmd[0] @($pythonCmd[1..($pythonCmd.Length - 1)]) @cmd
} else {
  & $pythonCmd[0] @cmd
}
if ($LASTEXITCODE -ne 0) {
  throw "perf gate failed with exit code $LASTEXITCODE"
}
