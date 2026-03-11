<##
Unsafe audit (Windows / PowerShell).

Policy:
1) Every `unsafe` in `crates/**/*.rs` must carry a nearby `// SAFETY:` comment.
2) `docs/UNSAFE_REGISTRY.md` is the single source of truth for unsafe file inventory.
3) Audit must fail on both missing registry entries and stale registry entries.

Self-check idea (negative case):
- Temporarily add an `unsafe { ... }` in any Rust file without `// SAFETY:` above it.
- Run `./scripts/unsafe_audit.ps1` and confirm it fails with `UNDOCUMENTED UNSAFE`.
- Revert the temporary change after validation.
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$registry = "docs\\UNSAFE_REGISTRY.md"
if (-not (Test-Path $registry)) {
  throw "missing $registry"
}

function Assert-UnsafeDocumented {
  param(
    [Parameter(Mandatory=$true)][string]$Path,
    [Parameter(Mandatory=$true)][string]$RelativePath
  )

  $lines = Get-Content -LiteralPath $Path
  $ring = New-Object System.Collections.Generic.Queue[string]
  $ringCap = 8

  for ($i = 0; $i -lt $lines.Count; $i++) {
    $line = $lines[$i]

    if ($ring.Count -ge $ringCap) {
      $null = $ring.Dequeue()
    }
    $ring.Enqueue($line)

    if ($line -match '(^|[^0-9A-Za-z_])unsafe\s*(\{|fn)') {
      $ok = $false
      foreach ($prev in $ring) {
        if ($prev -match '//\s*SAFETY:') {
          $ok = $true
          break
        }
      }

      if (-not $ok) {
        throw ("UNDOCUMENTED UNSAFE: {0}:{1}: {2}" -f $RelativePath, ($i + 1), $line.Trim())
      }
    }
  }
}

$unsafePat = [regex]'(^|[^0-9A-Za-z_])unsafe\s*(\{|fn)'
$unsafeFiles = New-Object System.Collections.Generic.HashSet[string]

Get-ChildItem -Path "crates" -Recurse -Filter "*.rs" | ForEach-Object {
  $path = $_.FullName
  $rel = [System.IO.Path]::GetRelativePath((Get-Location).Path, $path).Replace('\\', '/')
  $matches = Select-String -Path $path -Pattern $unsafePat -AllMatches -ErrorAction SilentlyContinue
  if ($matches) {
    $null = $unsafeFiles.Add($rel)
    Assert-UnsafeDocumented -Path $path -RelativePath $rel
  }
}

$registryFiles = New-Object System.Collections.Generic.HashSet[string]
Get-Content -LiteralPath $registry | ForEach-Object {
  if ($_ -match '^##\s+(crates/.+)$') {
    $null = $registryFiles.Add($Matches[1].Trim())
  }
}

foreach ($f in $unsafeFiles) {
  if (-not $registryFiles.Contains($f)) {
    throw "unsafe registry missing file entry: $f"
  }
}

foreach ($f in $registryFiles) {
  if (-not (Test-Path $f)) {
    throw "unsafe registry references missing file: $f"
  }
  if (-not $unsafeFiles.Contains($f)) {
    throw "unsafe registry stale entry (no unsafe in file): $f"
  }
}

Write-Host "OK: unsafe audit passed (documented + registry-synced)." -ForegroundColor Green
