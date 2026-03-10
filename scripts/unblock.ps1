<#[
递归解除“来自 Internet 下载的 zip”带来的 Mark-of-the-Web（Zone.Identifier）。

用法：
  PS> .\scripts\unblock.ps1

说明：
  - PowerShell 对带 Zone.Identifier 的脚本会在运行前弹出安全提示。
  - 该脚本会扫描仓库下所有文件，找到带 Zone.Identifier 的文件并执行 Unblock-File。
  - 建议首次解压 zip 后运行一次。
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$root = Resolve-Path "."

$blocked = @()
Get-ChildItem -Path $root -Recurse -File | ForEach-Object {
    $p = $_.FullName
    $hasZone = Get-Item -LiteralPath $p -Stream Zone.Identifier -ErrorAction SilentlyContinue
    if ($hasZone) {
        $blocked += $p
    }
}

if ($blocked.Count -eq 0) {
    Write-Host "OK: no blocked files found." -ForegroundColor Green
    exit 0
}

foreach ($p in $blocked) {
    try {
        Unblock-File -LiteralPath $p
    } catch {
        # Best-effort: ignore files that cannot be unblocked.
    }
}

Write-Host ("OK: unblocked {0} files." -f $blocked.Count) -ForegroundColor Green
