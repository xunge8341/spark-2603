#!/usr/bin/env bash
set -euo pipefail

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo not found"
  exit 1
fi

if ! cargo clippy --version >/dev/null 2>&1; then
  echo "clippy not installed. Tip: rustup component add clippy"
  exit 1
fi

# 主干五件套 + workspace tests
cargo clippy --workspace --all-targets --locked -- -D warnings
