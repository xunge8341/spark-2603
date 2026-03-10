<#
Run the local dataplane perf smoke.

Purpose:
- provide a stable, zero-dependency local baseline;
- print throughput + syscall batching ratios for the TX hot path;
- keep perf checks separate from the default verify gate.
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Invoke-Step {
  param(
    [Parameter(Mandatory=$true)][string]$Label,
    [Parameter(Mandatory=$true)][scriptblock]$Cmd
  )

  Write-Host $Label -ForegroundColor Cyan
  & $Cmd

  if ($LASTEXITCODE -ne 0) {
    throw "Step failed ($Label) with exit code $LASTEXITCODE"
  }
}

Invoke-Step "[1/2] cargo test -p spark-transport-contract --test perf_baseline --no-run" {
  cargo test -p spark-transport-contract --test perf_baseline --no-run
}

Invoke-Step "[2/2] cargo test -p spark-transport-contract --test perf_baseline -- --ignored --nocapture" {
  cargo test -p spark-transport-contract --test perf_baseline -- --ignored --nocapture
}

Write-Host "Note: SPARK_PERF line now includes alloc evidence (outbound/cumulation growth + peak)." -ForegroundColor DarkCyan
Write-Host "OK: perf baseline completed." -ForegroundColor Green
