<#
一键“修复”脚本（本地用）：
- 自动应用 rustfmt（写回文件）。
- 然后建议再跑 verify。

用法：
  PS> .\scripts\fix.ps1
  PS> .\scripts\verify.ps1
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

Write-Host "[1/1] cargo fmt (apply)" -ForegroundColor Cyan
& cargo fmt --all
if ($LASTEXITCODE -ne 0) {
  throw "cargo fmt failed with exit code $LASTEXITCODE"
}

Write-Host "OK: formatting applied. Next: .\\scripts\\verify.ps1" -ForegroundColor Green
