#!/usr/bin/env bash
set -euo pipefail

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo not found"
  exit 1
fi

if ! cargo fmt --version >/dev/null 2>&1; then
  echo "rustfmt not installed. Tip: rustup component add rustfmt"
  exit 1
fi

# rustfmt check mode; fail on diffs.
cargo fmt --all -- --check
