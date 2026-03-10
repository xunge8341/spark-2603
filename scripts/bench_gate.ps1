<#!
Run the mio TCP echo micro-bench and fail if coarse baseline thresholds regress.

Threshold resolution order:
1) Baseline file (`SPARK_BENCH_BASELINE`, else platform default under `perf/baselines/`)
2) Explicit env var overrides:
   - SPARK_BENCH_MIN_RPS
   - SPARK_BENCH_MAX_P99_US
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

function Resolve-BaselinePath {
  $explicit = [Environment]::GetEnvironmentVariable("SPARK_BENCH_BASELINE")
  if (-not [string]::IsNullOrWhiteSpace($explicit)) {
    return $explicit
  }

  $platformFile = "bench_tcp_echo_unix.toml"
  $sparkIsWindows = ($env:OS -eq 'Windows_NT')
  if ($sparkIsWindows) {
    $platformFile = "bench_tcp_echo_windows.toml"
  }

  $platformPath = Join-Path "perf\baselines" $platformFile
  if (Test-Path $platformPath) {
    return $platformPath
  }

  return (Join-Path "perf\baselines" "bench_tcp_echo_default.toml")
}

function Get-BaselineValue {
  param(
    [Parameter(Mandatory=$true)][string]$Path,
    [Parameter(Mandatory=$true)][string]$Key,
    [Parameter(Mandatory=$true)][string]$Default
  )

  if (-not (Test-Path $Path)) {
    return $Default
  }

  $content = Get-Content -Path $Path -Raw
  $pattern = '(?m)^\s*' + [regex]::Escape($Key) + '\s*=\s*(.+?)\s*$'
  $match = [regex]::Match($content, $pattern)
  if (-not $match.Success) {
    return $Default
  }

  return $match.Groups[1].Value.Trim().Trim('"')
}

function Get-EnvOrConfig {
  param(
    [Parameter(Mandatory=$true)][string]$Name,
    [Parameter(Mandatory=$true)][string]$Fallback
  )

  $value = [Environment]::GetEnvironmentVariable($Name)
  if ([string]::IsNullOrWhiteSpace($value)) {
    return $Fallback
  }
  return $value
}

$BaselinePath = Resolve-BaselinePath
$BaselineMinRps = Get-BaselineValue -Path $BaselinePath -Key "min_rps" -Default "1000"
$BaselineMaxP99Us = Get-BaselineValue -Path $BaselinePath -Key "max_p99_us" -Default "50000"
$BaselineProfile = Get-BaselineValue -Path $BaselinePath -Key "profile" -Default "default"

$MinRps = [double](Get-EnvOrConfig -Name "SPARK_BENCH_MIN_RPS" -Fallback $BaselineMinRps)
$MaxP99Us = [double](Get-EnvOrConfig -Name "SPARK_BENCH_MAX_P99_US" -Fallback $BaselineMaxP99Us)

Write-Host "Using bench baseline: $BaselinePath (profile=$BaselineProfile, min_rps=$MinRps, max_p99_us=$MaxP99Us)" -ForegroundColor DarkGray

Invoke-Step "[1/2] cargo test -p spark-transport-mio --test bench_tcp_echo --no-run --locked" {
  cargo test -p spark-transport-mio --test bench_tcp_echo --no-run --locked
}

Write-Host "[2/2] cargo test -p spark-transport-mio --test bench_tcp_echo -- --ignored --nocapture" -ForegroundColor Cyan
$cmd = 'cargo test -p spark-transport-mio --locked --test bench_tcp_echo -- --ignored --nocapture 2>&1'
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
$benchLine = $benchLine.Trim()

$rpsMatch = [regex]::Match($benchLine, 'rps=([0-9]+(?:\.[0-9]+)?)')
$p99Match = [regex]::Match($benchLine, 'p99_us=([0-9]+)')
if (-not $rpsMatch.Success -or -not $p99Match.Success) {
  throw "Failed to parse bench metrics from: $benchLine"
}

$rps = [double]$rpsMatch.Groups[1].Value
$p99 = [double]$p99Match.Groups[1].Value

if ($rps -lt $MinRps) {
  throw "Bench gate failed: rps=$rps is below $MinRps"
}

if ($p99 -gt $MaxP99Us) {
  throw "Bench gate failed: p99_us=$p99 exceeds $MaxP99Us"
}

Write-Host "OK: bench gate passed (profile=$BaselineProfile, rps=$rps, p99_us=$p99)." -ForegroundColor Green
