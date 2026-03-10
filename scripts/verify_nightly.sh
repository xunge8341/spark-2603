#!/usr/bin/env bash
set -euo pipefail

# Nightly verification wrapper (Linux/macOS/WSL).
#
# Keeps PR checks lightweight (see scripts/verify.sh) while enabling the
# heavier perf/bench gates in nightly CI.

export SPARK_VERIFY_PERF_GATE=1
export SPARK_VERIFY_BENCH_GATE=1
# Completion gate is a no-op on non-Windows platforms, but keeping it enabled
# avoids divergence between scripts.
export SPARK_VERIFY_COMPLETION_GATE=1

./scripts/verify.sh

# Foundational cross-target compile gates (no_std/alloc).
./scripts/nostd_gate.sh
