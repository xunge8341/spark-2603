#!/usr/bin/env bash
set -euo pipefail

echo "[1/1] cargo fmt (apply)"
cargo fmt --all

echo "OK: formatting applied. Next: ./scripts/verify.sh"
