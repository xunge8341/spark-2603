<#!
Run the mio TCP echo micro-bench (ignored test) and print a machine-readable summary line.

Environment variables:
- SPARK_BENCH_CONCURRENCY (default: 32)
- SPARK_BENCH_REQS_PER_CONN (default: 256)
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

Invoke-Step "[1/2] cargo test -p spark-transport-mio --test bench_tcp_echo --no-run" {
  cargo test -p spark-transport-mio --test bench_tcp_echo --no-run
}

Write-Host "[2/2] cargo test -p spark-transport-mio --test bench_tcp_echo -- --ignored --nocapture" -ForegroundColor Cyan
$cmd = 'cargo test -p spark-transport-mio --test bench_tcp_echo -- --ignored --nocapture 2>&1'
$raw = & cmd.exe /d /c $cmd
if ($LASTEXITCODE -ne 0) {
  throw "Step failed ([2/2] cargo test -p spark-transport-mio --test bench_tcp_echo -- --ignored --nocapture) with exit code $LASTEXITCODE"
}

$lines = @($raw)
$lines | ForEach-Object { Write-Host $_ }

$benchLine = $lines |
  ForEach-Object { $_ -split "`r?`n" } |
  Where-Object { $_ -match '^\s*SPARK_BENCH ' } |
  Select-Object -Last 1
if (-not $benchLine) {
  throw "Failed to find SPARK_BENCH output line."
}

Write-Host "OK: bench completed." -ForegroundColor Green
