<#
no_std/alloc compile gate for L0/L1 crates (Windows / PowerShell).

Goal:
- Ensure foundational crates stay compatible with embedded/WASM targets.
- Catch accidental `std` regressions early.

Intended use: nightly CI (see scripts/verify_nightly.ps1).
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

$Targets = @(
  "thumbv7em-none-eabihf",
  "wasm32-unknown-unknown"
)

$Crates = @(
  "spark-uci",
  "spark-core",
  "spark-buffer",
  "spark-codec",
  "spark-codec-text",
  "spark-codec-sip"
)

Invoke-Step "[no_std] rustup target add" {
  foreach ($t in $Targets) {
    rustup target add $t | Out-Null
  }
}

foreach ($t in $Targets) {
  foreach ($c in $Crates) {
    Invoke-Step ("[no_std] cargo build -p {0} --target {1}" -f $c, $t) {
      cargo build -p $c --target $t
    }
  }
}

Write-Host "OK: no_std/alloc target gate passed." -ForegroundColor Green
