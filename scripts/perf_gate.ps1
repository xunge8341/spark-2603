<#
Run the local dataplane perf smoke and fail if coarse baseline thresholds regress.

Threshold resolution order:
1) Baseline file (`SPARK_PERF_BASELINE`, else platform default under `perf/baselines/`)
2) Explicit env var overrides:
   - SPARK_MAX_SYSCALLS_PER_KIB
   - SPARK_MIN_WRITEV_SHARE
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
  $explicit = [Environment]::GetEnvironmentVariable("SPARK_PERF_BASELINE")
  if (-not [string]::IsNullOrWhiteSpace($explicit)) {
    return $explicit
  }

  $platformFile = "unix.toml"
  $sparkIsWindows = ($env:OS -eq 'Windows_NT')
  if ($sparkIsWindows) {
    $platformFile = "windows.toml"
  }

  $platformPath = Join-Path "perf\baselines" $platformFile
  if (Test-Path $platformPath) {
    return $platformPath
  }

  return (Join-Path "perf\baselines" "default.toml")
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
$BaselineMaxSyscallsPerKiB = Get-BaselineValue -Path $BaselinePath -Key "max_syscalls_per_kib" -Default "1.0"
$BaselineMinWritevShare = Get-BaselineValue -Path $BaselinePath -Key "min_writev_share" -Default "0.5"
$BaselineProfile = Get-BaselineValue -Path $BaselinePath -Key "profile" -Default "default"

$MaxSyscallsPerKiB = [double](Get-EnvOrConfig -Name "SPARK_MAX_SYSCALLS_PER_KIB" -Fallback $BaselineMaxSyscallsPerKiB)
$MinWritevShare = [double](Get-EnvOrConfig -Name "SPARK_MIN_WRITEV_SHARE" -Fallback $BaselineMinWritevShare)

Write-Host "Using perf baseline: $BaselinePath (profile=$BaselineProfile, max_syscalls_per_kib=$MaxSyscallsPerKiB, min_writev_share=$MinWritevShare)" -ForegroundColor DarkGray

Invoke-Step "[1/2] cargo test -p spark-transport-contract --test perf_baseline --no-run --locked" {
  cargo test -p spark-transport-contract --test perf_baseline --no-run --locked
}

Write-Host "[2/2] cargo test -p spark-transport-contract --test perf_baseline -- --ignored --nocapture" -ForegroundColor Cyan
$cmd = 'cargo test -p spark-transport-contract --locked --test perf_baseline -- --ignored --nocapture 2>&1'
$raw = & cmd.exe /d /c $cmd
if ($LASTEXITCODE -ne 0) {
  throw "Step failed ([2/2] cargo test -p spark-transport-contract --test perf_baseline -- --ignored --nocapture) with exit code $LASTEXITCODE"
}

$lines = @($raw)
$lines | ForEach-Object { Write-Host $_ }

$perfLine = $lines |
  ForEach-Object { $_ -split "`r?`n" } |
  Where-Object { $_ -match '^\s*SPARK_PERF ' } |
  Select-Object -Last 1
if (-not $perfLine) {
  throw "Failed to find SPARK_PERF output line."
}
$perfLine = $perfLine.Trim()

$syscallsPerKiBMatch = [regex]::Match($perfLine, 'syscalls_per_kib=([0-9]+(?:\.[0-9]+)?)')
$writevShareMatch = [regex]::Match($perfLine, 'writev_share=([0-9]+(?:\.[0-9]+)?)')
if (-not $syscallsPerKiBMatch.Success -or -not $writevShareMatch.Success) {
  throw "Failed to parse perf metrics from: $perfLine"
}

$syscallsPerKiB = [double]$syscallsPerKiBMatch.Groups[1].Value
$writevShare = [double]$writevShareMatch.Groups[1].Value

if ($syscallsPerKiB -gt $MaxSyscallsPerKiB) {
  throw "Perf gate failed: syscalls_per_kib=$syscallsPerKiB exceeds $MaxSyscallsPerKiB"
}

if ($writevShare -lt $MinWritevShare) {
  throw "Perf gate failed: writev_share=$writevShare is below $MinWritevShare"
}

Write-Host "OK: perf gate passed (profile=$BaselineProfile, syscalls_per_kib=$syscallsPerKiB, writev_share=$writevShare)." -ForegroundColor Green
