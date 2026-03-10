#!/usr/bin/env bash
set -euo pipefail

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo not found; skipping dependency checks."
  exit 0
fi

# `cargo tree` comes from the `cargo-tree` component and may be missing.
if ! cargo tree -V >/dev/null 2>&1; then
  echo "cargo-tree not installed; skipping dependency checks."
  echo "Tip: run: cargo install cargo-tree"
  exit 0
fi

# 1) spark-core 必须保持最底层（不依赖 transport/backends）。
if cargo tree -p spark-core --edges normal 2>/dev/null | grep -E "spark-transport|spark-transport-mio|spark-engine" >/dev/null; then
  echo "ERROR: spark-core violates isolation; it must NOT depend on transport/backends."
  exit 1
fi

# 2) spark-transport 必须保持运行时中立（不依赖 tokio/mio/后端实现）。
if cargo tree -p spark-transport --edges normal 2>/dev/null | grep -E "tokio|mio|spark-transport-mio|spark-engine" >/dev/null; then
  echo "ERROR: spark-transport must remain runtime-neutral; it must NOT depend on tokio/mio/backends."
  exit 1
fi

# 2b) spark-transport 必须保持“零 adaptor”（不直接依赖 log/tracing 等）。
if cargo tree -p spark-transport --edges normal 2>/dev/null | grep -E "(^|\s)log\s|tracing" >/dev/null; then
  echo "ERROR: spark-transport must not depend on log/tracing; adaptors belong to leaf crates only."
  exit 1
fi

# 3) spark-host 必须保持运行时中立。
if cargo tree -p spark-host --edges normal 2>/dev/null | grep -E "tokio|mio|spark-transport-mio|spark-engine" >/dev/null; then
  echo "ERROR: spark-host must remain runtime-neutral; it must NOT depend on tokio/mio/backends."
  exit 1
fi

# 4) spark-core 源码禁止 unsafe（best-effort）
if command -v rg >/dev/null 2>&1; then
  if rg -n "\\bunsafe\\b" crates/spark-core/src >/dev/null; then
    echo "ERROR: spark-core must contain no unsafe."
    exit 1
  fi
else
  if grep -R "\\bunsafe\\b" crates/spark-core/src >/dev/null 2>&1; then
    echo "ERROR: spark-core must contain no unsafe."
    exit 1
  fi
fi

# 5) spark-transport 热路径禁止 downcast（仅允许 diagnostics helpers）
if command -v rg >/dev/null 2>&1; then
  hits=$(rg -n "\\.downcast_" crates/spark-transport/src || true)
else
  hits=$(grep -R -n "\.downcast_" crates/spark-transport/src 2>/dev/null || true)
fi

if [[ -n "${hits}" ]]; then
  bad=$(echo "${hits}" | grep -vE "crates/spark-transport/src/async_bridge/channel_driver\\.rs" || true)
  if [[ -n "${bad}" ]]; then
    echo "ERROR: spark-transport must not use downcast in hot path (only diagnostics helpers are allowed)."
    echo "--- offending occurrences ---"
    echo "${bad}"
    exit 1
  fi

  # Ensure the allowed helper stays behind cfg.
  if ! grep -q "cfg(any(test, feature = \"diagnostics\"))" crates/spark-transport/src/async_bridge/channel_driver.rs; then
    echo "ERROR: downcast helper in channel_driver.rs must be gated behind #[cfg(any(test, feature = \"diagnostics\"))]"
    exit 1
  fi
fi

echo "OK: Architectural invariants passed."
