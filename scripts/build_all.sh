#!/usr/bin/env bash
set -euo pipefail

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo not found; run this script in an environment with Rust toolchain." >&2
  exit 1
fi

./scripts/check_deps.sh

cargo fmt --all
cargo test --workspace
