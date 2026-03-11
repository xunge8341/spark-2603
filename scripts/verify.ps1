<#
面向 Windows / PowerShell 的一键验证脚本。

目标：
- 把 `spark-transport-contract` 当作主干 gate（语义不回退）。
- 把“编译/文档警告”视作失败（-D warnings）。
- 固化为 CI/本地规范：PR 合并前至少跑一次。

用法：
  PS> .\scripts\verify.ps1

说明：
  - 默认会跑 fmt / clippy / invariants / panic-free scan / L0 unsafe scan / contract suite / test。
  - perf gate 默认不开启（避免日常 PR/本地验证时长飘移）；nightly 脚本默认开启。
  - 若设置 `SPARK_VERIFY_PERF_GATE=1`，结尾会追加一次 perf gate。
  - 若设置 `SPARK_VERIFY_BENCH_GATE=1`，结尾会追加一次 mio TCP echo bench gate。
  - 若设置 `SPARK_VERIFY_COMPLETION_GATE=1`，会额外编译 completion 原型（IOCP）。
  - perf gate 默认读取 `perf/baselines/` 下的平台基线，可用 `SPARK_PERF_BASELINE` 覆盖。
  - bench gate 默认读取 `perf/baselines/bench_tcp_echo_*` 下的平台基线，可用 `SPARK_BENCH_BASELINE` 覆盖。
  - 如果你的环境缺少 rustfmt/clippy，请先执行：
      rustup component add rustfmt clippy
  - 如果缺少 cargo-tree：
      cargo install cargo-tree
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Invoke-Step {
  param(
    [Parameter(Mandatory=$true)][string]$Label,
    [Parameter(Mandatory=$true)][scriptblock]$Cmd
  )

  Write-Host $Label -ForegroundColor Cyan
  $global:LASTEXITCODE = 0
  & $Cmd

  # Important: external commands do NOT honor $ErrorActionPreference.
  if ($LASTEXITCODE -ne 0) {
    throw "Step failed ($Label) with exit code $LASTEXITCODE"
  }
}

# Treat rustc warnings as errors (best-effort).
$env:RUSTFLAGS = "-D warnings"
$env:RUSTDOCFLAGS = "-D warnings"

$RunPerfGate = [Environment]::GetEnvironmentVariable("SPARK_VERIFY_PERF_GATE")
$RunPerfGate = -not [string]::IsNullOrWhiteSpace($RunPerfGate) -and @("1", "true", "yes", "on") -contains $RunPerfGate.ToLowerInvariant()

$RunBenchGate = [Environment]::GetEnvironmentVariable("SPARK_VERIFY_BENCH_GATE")
$RunBenchGate = -not [string]::IsNullOrWhiteSpace($RunBenchGate) -and @("1", "true", "yes", "on") -contains $RunBenchGate.ToLowerInvariant()

$RunCompletionGate = [Environment]::GetEnvironmentVariable("SPARK_VERIFY_COMPLETION_GATE")
$RunCompletionGate = -not [string]::IsNullOrWhiteSpace($RunCompletionGate) -and @("1", "true", "yes", "on") -contains $RunCompletionGate.ToLowerInvariant()

Write-Host "[status] Linux dataplane baseline: spark-transport + mio" -ForegroundColor DarkGray
Write-Host "[status] Windows IOCP phase-0 compatibility layer: wrapper mode default; native completion opt-in" -ForegroundColor DarkGray
Write-Host "[known-gap] write_pressure_smoke: Windows forward-progress stall (tracked, expected known-failing)" -ForegroundColor Yellow

Invoke-Step "[1/8] cargo fmt" {
  # Use rustfmt's check mode; fail on diffs.
  cargo fmt --all -- --check
}

Invoke-Step "[2/8] cargo clippy (deny warnings)" {
  cargo clippy --workspace --all-targets --locked -- -D warnings
}

Write-Host "[3/8] architectural invariants" -ForegroundColor Cyan
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
  throw "cargo not found"
}


function Invoke-PanicFreeScan {
  # Best-effort panic-free gate for core crates (Windows / PowerShell).
  #
  # Policy:
  # - In non-test code, forbid: unwrap/expect/panic!/todo!/unimplemented!/unreachable!
  # - Allow those patterns inside #[cfg(test)] mod blocks (heuristic), and in files under */tests/.
  $crates = @(
    "crates\spark-uci\src",
    "crates\spark-core\src",
    "crates\spark-buffer\src",
    "crates\spark-codec\src",
    "crates\spark-transport\src",
    "crates\spark-host\src",
    "crates\spark-ember\src"
  )

  $pat = [regex]"(\.unwrap\(\)|\.expect\(|\bpanic!\(|\btodo!\(|\bunimplemented!\(|\bunreachable!\()"
  $cfgTest = [regex]"^\s*#\[cfg\(test\)\]\s*$"
  $modStart = [regex]"^\s*mod\s+\w+\s*\{\s*$"

  $bad = New-Object System.Collections.Generic.List[object]

  foreach ($src in $crates) {
    if (-not (Test-Path $src)) { continue }
    Get-ChildItem -Path $src -Recurse -Filter "*.rs" | ForEach-Object {
      $path = $_.FullName
      $rel = Resolve-Path -Relative $path

      # Skip anything under */tests/ (not src/, but keep the guard).
      if ($rel -match "\\tests\\") { return }

      $lines = Get-Content -LiteralPath $path
      $inCfgTestMod = $false
      $braceDepth = 0
      $sawCfgTest = $false

      for ($i = 0; $i -lt $lines.Count; $i++) {
        $line = $lines[$i]

        if (-not $inCfgTestMod -and $cfgTest.IsMatch($line)) {
          $sawCfgTest = $true
          continue
        }

        if ($sawCfgTest -and -not $inCfgTestMod) {
          if ($modStart.IsMatch($line)) {
            $inCfgTestMod = $true
            $braceDepth = 1
            $sawCfgTest = $false
            continue
          }

          $trim = $line.Trim()
          if ($trim.StartsWith("#") -or $trim -eq "") {
            continue
          }
          $sawCfgTest = $false
        }

        if ($inCfgTestMod) {
          $braceDepth += ([regex]::Matches($line, "\{").Count)
          $braceDepth -= ([regex]::Matches($line, "\}").Count)
          if ($braceDepth -le 0) {
            $inCfgTestMod = $false
            $braceDepth = 0
          }
          continue
        }

        if ($pat.IsMatch($line)) {
          $bad.Add([pscustomobject]@{ Path = $rel; Line = $i + 1; Snippet = $line.Trim() }) | Out-Null
        }
      }
    }
  }

  if ($bad.Count -gt 0) {
    Write-Host "ERROR: panic-free gate failed (non-test code contains panic-y constructs):" -ForegroundColor Red
    $bad | Select-Object -First 200 | ForEach-Object {
      Write-Host ("  {0}:{1}: {2}" -f $_.Path, $_.Line, $_.Snippet) -ForegroundColor Red
    }
    if ($bad.Count -gt 200) {
      Write-Host ("  ... and {0} more" -f ($bad.Count - 200)) -ForegroundColor Red
    }
    throw "panic-free scan failed"
  }
}

function Invoke-L0UnsafeScan {
  # Ensure L0 crates stay free of unsafe blocks and extern C.
  $l0 = @(
    "crates\spark-core\src",
    "crates\spark-uci\src"
  )
  $unsafePat = [regex]'\bunsafe\s*(\{|fn)\b'
  $externCPat = [regex]'extern\s+"C"'

  $hits = New-Object System.Collections.Generic.List[object]
  foreach ($src in $l0) {
    if (-not (Test-Path $src)) { continue }
    Get-ChildItem -Path $src -Recurse -Filter "*.rs" | ForEach-Object {
      $path = $_.FullName
      $rel = Resolve-Path -Relative $path
      $m1 = Select-String -Path $path -Pattern $unsafePat -AllMatches -ErrorAction SilentlyContinue
      if ($m1) {
        $m1 | ForEach-Object { $hits.Add([pscustomobject]@{ Path = $rel; Line = $_.LineNumber; Snippet = $_.Line.Trim() }) | Out-Null }
      }
      $m2 = Select-String -Path $path -Pattern $externCPat -AllMatches -ErrorAction SilentlyContinue
      if ($m2) {
        $m2 | ForEach-Object { $hits.Add([pscustomobject]@{ Path = $rel; Line = $_.LineNumber; Snippet = $_.Line.Trim() }) | Out-Null }
      }
    }
  }

  if ($hits.Count -gt 0) {
    Write-Host "ERROR: L0 unsafe scan failed (spark-core/spark-uci must remain unsafe-free):" -ForegroundColor Red
    $hits | Select-Object -First 200 | ForEach-Object {
      Write-Host ("  {0}:{1}: {2}" -f $_.Path, $_.Line, $_.Snippet) -ForegroundColor Red
    }
    throw "L0 unsafe scan failed"
  }
}

Invoke-Step "[4/8] panic-free scan (core crates)" {
  Invoke-PanicFreeScan
}

Invoke-Step "[5/8] L0 unsafe scan (spark-core/spark-uci)" {
  Invoke-L0UnsafeScan
}

$hasTree = $false
& cargo tree -V | Out-Null
if ($LASTEXITCODE -eq 0) { $hasTree = $true }

if (-not $hasTree) {
  Write-Host "cargo-tree not installed; skipping dependency checks. Tip: cargo install cargo-tree" -ForegroundColor Yellow
} else {
  # Isolation invariants (best-effort, still strict when cargo-tree exists)
  if (cargo tree -p spark-core | Select-String -Pattern "spark-transport|spark-transport-mio") {
    throw "spark-core must not depend on transport/backends"
  }
  if (cargo tree -p spark-transport | Select-String -Pattern "tokio|mio|spark-transport-mio") {
    throw "spark-transport must remain runtime-neutral"
  }
  if (cargo tree -p spark-transport | Select-String -Pattern "(^|\s)log\s|tracing") {
    throw "spark-transport must not depend on log/tracing; adaptors belong to leaf crates only"
  }
  if (cargo tree -p spark-host | Select-String -Pattern "tokio|mio|spark-transport-mio") {
    throw "spark-host must remain runtime-neutral"
  }
}

# Hot-path invariants: forbid downcast in spark-transport (except diagnostics helpers).
# Use an explicit file list instead of globbing (more reliable across PS versions).
$transportSrc = Get-ChildItem -Path "crates\spark-transport\src" -Recurse -Filter "*.rs" | ForEach-Object { $_.FullName }
$downcastHits = $transportSrc | Select-String -Pattern "\.downcast_" -AllMatches
if ($downcastHits) {
  $bad = $downcastHits | Where-Object {
    $_.Path -notmatch "\\bridge\.rs$" -and $_.Path -notmatch "\\channel_driver\.rs$"
  }
  if ($bad) {
    $bad | ForEach-Object { Write-Host ("DOWNCAST FORBIDDEN: {0}:{1}: {2}" -f $_.Path, $_.LineNumber, $_.Line) -ForegroundColor Red }
    throw "spark-transport must not use downcast in hot path (only diagnostics helpers are allowed)"
  }

  # Ensure the allowed helpers stay behind cfg.
  # PowerShell does NOT support C-style escaping (\") inside double-quoted strings.
  # Use single-quoted literals to keep the Rust snippet intact and avoid parser errors.
  # Be tolerant to whitespace variations.
  $cfgPattern = 'cfg\(\s*any\(\s*test\s*,\s*feature\s*=\s*"diagnostics"\s*\)\s*\)'
  $cfgHint = '#[cfg(any(test, feature = "diagnostics"))]'
  $needCfg = @(
    "crates\spark-transport\src\bridge.rs",
    "crates\spark-transport\src\async_bridge\channel_driver.rs"
  )
  foreach ($p in $needCfg) {
    if (-not (Select-String -Path $p -Pattern $cfgPattern)) {
      throw "downcast helper in $p must be gated behind $cfgHint"
    }
  }
}


Invoke-Step "[5b/8] unsafe audit (documented + registry-synced)" {
  & .\scripts\unsafe_audit.ps1
}

Invoke-Step "[6/8] cargo test --workspace --no-run --locked (compile gate)" {
  cargo test --workspace --no-run --locked
}

Invoke-Step "[7/8] contract suite gate (spark-transport-contract)" {
  cargo test -p spark-transport-contract --locked
}

Invoke-Step "[8/8] cargo test --workspace (excluding contract suite)" {
  cargo test --workspace --exclude spark-transport-contract --locked
}

if ($RunCompletionGate) {
  Invoke-Step "[optional] completion prototype compile (SPARK_VERIFY_COMPLETION_GATE)" {
    # Runnable gate: ensures the native IOCP completion prototype does not bit-rot.
    #
    # DECISION (BigStep-19): compile-only is insufficient; we validate the minimal submit → completion → poll closure
    # (posted completion packets) in addition to port creation + empty polling.
    cargo test -p spark-transport-iocp --features native-completion --locked
  }
}

if ($RunPerfGate) {
  Invoke-Step "[optional] perf gate (SPARK_VERIFY_PERF_GATE)" {
    & .\scripts\perf_gate.ps1
  }
}

if ($RunBenchGate) {
  Invoke-Step "[optional] bench gate (SPARK_VERIFY_BENCH_GATE)" {
    & .\scripts\bench_gate.ps1
  }
}

Write-Host "OK: verify passed." -ForegroundColor Green
