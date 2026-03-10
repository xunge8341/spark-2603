#!/usr/bin/env bash
set -euo pipefail

echo "[deny] cargo-deny (optional gate)"

if ! command -v cargo-deny >/dev/null 2>&1; then
  echo "cargo-deny not found. Install with: cargo install cargo-deny --locked" >&2
  exit 1
fi

cargo deny check
echo "OK: cargo deny check passed."
