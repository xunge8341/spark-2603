#!/usr/bin/env bash
set -euo pipefail

# no_std/alloc compile gate for L0/L1 crates.
#
# Goal:
# - Ensure the foundational crates stay compatible with embedded/WASM targets.
# - Catch accidental `std` regressions early.
#
# This gate is intended to run in nightly CI (see scripts/verify_nightly.*).

targets=(
  thumbv7em-none-eabihf
  wasm32-unknown-unknown
)

crates=(
  spark-uci
  spark-core
  spark-buffer
  spark-codec
  spark-codec-text
  spark-codec-sip
)

echo "Using targets: ${targets[*]}" >&2

for t in "${targets[@]}"; do
  rustup target add "$t" >/dev/null
done

for t in "${targets[@]}"; do
  echo "[no_std] target=$t" >&2
  for c in "${crates[@]}"; do
    echo "  cargo build -p $c --target $t" >&2
    cargo build -p "$c" --target "$t"
  done
done

echo "OK: no_std/alloc target gate passed." >&2
