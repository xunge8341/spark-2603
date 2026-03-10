<#!
Nightly verification wrapper for Windows / PowerShell.

Goals:
- Keep day-to-day PR verification fast (see scripts/verify.ps1).
- Make perf/bench regressions visible with a single, consistent entry point.

This script intentionally:
- enables perf + bench gates;
- enables completion compile gate (cheap, prevents bit-rot);
- delegates all real logic to verify.ps1.
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$env:SPARK_VERIFY_PERF_GATE = "1"
$env:SPARK_VERIFY_BENCH_GATE = "1"
$env:SPARK_VERIFY_COMPLETION_GATE = "1"

.\scripts\verify.ps1

.\scripts\nostd_gate.ps1
