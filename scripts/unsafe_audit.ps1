<##
Unsafe audit (Windows / PowerShell).

Policy:
1) In core spark-transport, unsafe must be confined to a small, auditable set of modules.
2) Every unsafe block/item must have a nearby `// SAFETY:` or `// Safety:` comment.
3) Leaf backends (e.g., IOCP) may use unsafe, but must still be documented.
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Assert-UnsafeDocumented {
  param(
    [Parameter(Mandatory=$true)][string]$Path
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
        if ($prev -match '//\s*(SAFETY|Safety):') {
          $ok = $true
          break
        }
      }

      if (-not $ok) {
        $rel = Resolve-Path -Relative $Path
        throw ("UNDOCUMENTED UNSAFE: {0}:{1}: {2}" -f $rel, ($i + 1), $line.Trim())
      }
    }
  }
}

# 1) Confinement: spark-transport unsafe is allowed only in these modules.
$allow = @(
  "crates\spark-transport\src\lease.rs",
  "crates\spark-transport\src\reactor\event_buf.rs"
)

$unsafePat = [regex]'(^|[^0-9A-Za-z_])unsafe\s*(\{|fn)'
$transportSrc = Get-ChildItem -Path "crates\spark-transport\src" -Recurse -Filter "*.rs" | ForEach-Object { $_.FullName }

$hits = New-Object System.Collections.Generic.List[object]
foreach ($p in $transportSrc) {
  $m = Select-String -Path $p -Pattern $unsafePat -AllMatches -ErrorAction SilentlyContinue
  if ($m) {
    $m | ForEach-Object { $hits.Add([pscustomobject]@{ Path = $_.Path; Line = $_.LineNumber; Snippet = $_.Line.Trim() }) | Out-Null }
  }
}

if ($hits.Count -gt 0) {
  $bad = $hits | Where-Object {
    $ok = $false
    foreach ($a in $allow) {
      if ($_.Path -like ("*" + $a.Replace("/", "\"))) { $ok = $true; break }
    }
    -not $ok
  }

  if ($bad) {
    $bad | Select-Object -First 200 | ForEach-Object {
      Write-Host ("UNSAFE FORBIDDEN: {0}:{1}: {2}" -f $_.Path, $_.Line, $_.Snippet) -ForegroundColor Red
    }
    throw "unsafe must be confined to lease.rs and reactor/event_buf.rs (auditable modules)"
  }
}

# 2) Documentation: enforce SAFETY comments.
foreach ($p in $allow) {
  if (Test-Path $p) {
    Assert-UnsafeDocumented -Path $p
  }
}

# 3) Leaf IOCP backend: allow unsafe but require documentation.
if (Test-Path "crates\spark-transport-iocp\src") {
  Get-ChildItem -Path "crates\spark-transport-iocp\src" -Recurse -Filter "*.rs" | ForEach-Object {
    Assert-UnsafeDocumented -Path $_.FullName
  }
}

Write-Host "OK: unsafe audit passed (confined + documented)." -ForegroundColor Green
