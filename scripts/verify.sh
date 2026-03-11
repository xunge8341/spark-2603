#!/usr/bin/env bash
set -euo pipefail

die(){ echo "ERROR: $*" 1>&2; exit 1; }

# Treat rustc warnings as errors (best-effort).
# Set SPARK_VERIFY_PERF_GATE=1 to append the local perf gate at the end.
# Set SPARK_VERIFY_BENCH_GATE=1 to append the mio TCP echo bench gate at the end.
# Set SPARK_VERIFY_COMPLETION_GATE=1 to run the native IOCP completion prototype tests.
# The perf gate resolves thresholds from perf/baselines/ unless SPARK_PERF_BASELINE overrides it.
export RUSTFLAGS="-D warnings"
export RUSTDOCFLAGS="-D warnings"

run_perf_gate="${SPARK_VERIFY_PERF_GATE:-}"
run_perf_gate="${run_perf_gate,,}"

run_bench_gate="${SPARK_VERIFY_BENCH_GATE:-}"
run_bench_gate="${run_bench_gate,,}"

run_completion_gate="${SPARK_VERIFY_COMPLETION_GATE:-}"
run_completion_gate="${run_completion_gate,,}"

is_windows=0
case "${OSTYPE:-}" in
  msys*|cygwin*|win32*) is_windows=1 ;;
esac

print_backend_status(){
  echo "[status] Linux dataplane: production baseline (spark-transport-mio)"
  echo "[status] Windows IOCP backend: phase-0 compatibility layer (NOT native production dataplane)"
  echo "[status] Known gap: KI-001 Windows write_pressure_smoke forward-progress stall (known-failing)"
}

run_windows_known_failing_gate(){
  if [[ "$is_windows" -ne 1 ]]; then
    echo "[known-gap] windows_write_pressure_smoke: skipped (non-Windows environment)"
    return 0
  fi

  echo "[known-gap] windows_write_pressure_smoke: running ignored test as known-failing evidence"
  if cargo test -p spark-transport-mio --test write_pressure_smoke -- --ignored --nocapture; then
    die "windows_write_pressure_smoke unexpectedly passed; update KI-001 status and remove ignore marker"
  fi
  echo "[known-gap] windows_write_pressure_smoke: still failing (expected in current phase)."
}


print_backend_status

echo "[1/7] cargo fmt"
bash ./scripts/fmt.sh

echo "[2/7] cargo clippy (deny warnings)"
bash ./scripts/clippy.sh

echo "[3/7] architectural invariants"
bash ./scripts/check_deps.sh

echo "[4/7] panic-free scan (core crates)"
bash ./scripts/panic_scan.sh


echo "[4.5/7] unsafe audit (documented + confined)"
bash ./scripts/unsafe_audit.sh

echo "[5/7] cargo test --workspace --no-run --locked (compile gate)"
cargo test --workspace --no-run --locked

echo "[6/7] contract suite gate (spark-transport-contract)"
cargo test -p spark-transport-contract --locked

echo "[7/7] cargo test --workspace (excluding contract suite)"
cargo test --workspace --exclude spark-transport-contract --locked


if [[ "$run_completion_gate" == "1" || "$run_completion_gate" == "true" || "$run_completion_gate" == "yes" || "$run_completion_gate" == "on" ]]; then
  echo "[optional] completion prototype smoke (SPARK_VERIFY_COMPLETION_GATE)"
  # DECISION (BigStep-19): compile-only is insufficient; we validate the minimal submit -> completion -> poll closure
  # (posted completion packets) in addition to port creation + empty polling.
  cargo test -p spark-transport-iocp --features native-completion --locked
fi

if [[ "$run_perf_gate" == "1" || "$run_perf_gate" == "true" || "$run_perf_gate" == "yes" || "$run_perf_gate" == "on" ]]; then
  echo "[optional] perf gate (SPARK_VERIFY_PERF_GATE)"
  bash ./scripts/perf_gate.sh
fi

if [[ "$run_bench_gate" == "1" || "$run_bench_gate" == "true" || "$run_bench_gate" == "yes" || "$run_bench_gate" == "on" ]]; then
  echo "[optional] bench gate (SPARK_VERIFY_BENCH_GATE)"
  bash ./scripts/bench_gate.sh
fi


run_windows_known_failing_gate

echo "OK: verify passed."
