#!/usr/bin/env bash
set -euo pipefail

# Best-effort panic-free gate for core crates.
#
# Policy:
# - In non-test code, forbid: unwrap/expect/panic!/todo!/unimplemented!/unreachable!
# - Allow those patterns in:
#   - files under */tests/
#   - code inside #[cfg(test)] modules
#
# This gate is intentionally conservative and focuses on core crates.

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

CRATES=(
  "crates/spark-uci"
  "crates/spark-core"
  "crates/spark-buffer"
  "crates/spark-codec"
  "crates/spark-transport"
  "crates/spark-host"
  "crates/spark-ember"
)

python3 - <<'PY' "$ROOT" "${CRATES[@]}"
import re, sys
from pathlib import Path

root = Path(sys.argv[1])
crates = [Path(p) for p in sys.argv[2:]]

# Patterns that would panic in production.
pat = re.compile(r"(\\.unwrap\\(\\)|\\.expect\\(|\\bpanic!\\(|\\btodo!\\(|\\bunimplemented!\\(|\\bunreachable!\\()")

# Heuristic: ignore regions that are inside a `#[cfg(test)] mod ... {}` block.
# This is not a full Rust parser, but works for the prevailing code style in this repo.
cfg_test_start = re.compile(r"^\\s*#\\[cfg\\(test\\)\\]\\s*$")
mod_start = re.compile(r"^\\s*mod\\s+\\w+\\s*\\{\\s*$")

bad = []

for crate in crates:
    src = root / crate / "src"
    if not src.exists():
        continue
    for rs in sorted(src.rglob("*.rs")):
        # Skip generated / vendored paths if any.
        rel = rs.relative_to(root)
        text = rs.read_text(encoding="utf-8", errors="ignore").splitlines()

        in_cfg_test_mod = False
        brace_depth = 0
        saw_cfg_test = False

        for i, line in enumerate(text, start=1):
            # Enter cfg(test) module block.
            if not in_cfg_test_mod and cfg_test_start.match(line):
                saw_cfg_test = True
                continue
            if saw_cfg_test and not in_cfg_test_mod:
                # Expect `mod xxx {` soon after.
                if mod_start.match(line):
                    in_cfg_test_mod = True
                    brace_depth = 1
                    saw_cfg_test = False
                    continue
                # Allow attributes / empty lines between cfg and mod.
                if line.strip().startswith("#") or line.strip() == "":
                    continue
                # If something else appears, give up on pairing.
                saw_cfg_test = False

            if in_cfg_test_mod:
                brace_depth += line.count("{")
                brace_depth -= line.count("}")
                if brace_depth <= 0:
                    in_cfg_test_mod = False
                continue

            # Outside cfg(test) module: flag panic-y patterns.
            if pat.search(line):
                bad.append((str(rel), i, line.strip()))

if bad:
    print("ERROR: panic-free gate failed (non-test code contains panic-y constructs):")
    for path, line, snippet in bad[:200]:
        print(f"  {path}:{line}: {snippet}")
    if len(bad) > 200:
        print(f"  ... and {len(bad)-200} more")
    sys.exit(1)

print("OK: panic-free scan passed.")
PY
