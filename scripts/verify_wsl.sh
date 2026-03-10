#!/usr/bin/env bash
set -euo pipefail

# WSL/Ubuntu convenience wrapper.
#
# Notes:
# - If the repo lives on /mnt/<drive>/..., builds can be slower.
#   We default CARGO_TARGET_DIR to the Linux filesystem to speed up incremental builds.
# - PowerShell unblocking is not required on Linux/WSL.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ -f /proc/version ]] && grep -qi microsoft /proc/version; then
  echo "WSL detected: $(uname -r)"

  # WSL NAT mode cannot reach Windows localhost proxy endpoints (127.0.0.1/localhost)
  # from inside the Linux VM. If the user has a localhost proxy configured, it can
  # break `cargo` downloads. Prefer unsetting for this verification run.
  for v in http_proxy https_proxy HTTP_PROXY HTTPS_PROXY ALL_PROXY all_proxy; do
    val="${!v-}"
    if [[ -n "${val}" ]] && echo "${val}" | grep -Eqi '(^|://)(localhost|127\.0\.0\.1)(:|/|$)'; then
      echo "WSL: detected localhost proxy in \$${v}; unsetting for this run (NAT mode cannot reach Windows localhost)."
      unset "$v" || true
    fi
  done
else
  echo "Linux detected: $(uname -r)"
fi

# Prefer Linux filesystem for build artifacts.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/spark-2026/target}"
mkdir -p "$CARGO_TARGET_DIR" >/dev/null 2>&1 || true

chmod +x scripts/*.sh >/dev/null 2>&1 || true

echo "Using CARGO_TARGET_DIR=$CARGO_TARGET_DIR"

echo "[1/3] scripts/fix.sh"
./scripts/fix.sh

echo "[2/3] scripts/verify.sh"
./scripts/verify.sh

echo "[3/3] scripts/perf_gate.sh"
./scripts/perf_gate.sh

echo "OK: WSL/Linux verification passed."
