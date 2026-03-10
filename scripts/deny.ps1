$ErrorActionPreference = 'Stop'

Write-Host "[deny] cargo-deny (optional gate)" -ForegroundColor Cyan

if (-not (Get-Command cargo-deny -ErrorAction SilentlyContinue)) {
    throw "cargo-deny not found. Install with: cargo install cargo-deny --locked"
}

cargo deny check
Write-Host "OK: cargo deny check passed." -ForegroundColor Green
