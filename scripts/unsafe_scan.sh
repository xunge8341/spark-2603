#!/usr/bin/env bash
set -euo pipefail

OUT_PATH=${1:-provenance/unsafe_dependency_report.json}

python3 - <<'PY' "$OUT_PATH"
import json, re, sys, time
from pathlib import Path

out_path = Path(sys.argv[1])
root = Path.cwd()

crate_roots = []
for base in [root / "crates", root / "apps"]:
    if base.exists():
        for p in base.iterdir():
            if p.is_dir() and (p / "Cargo.toml").exists() and (p / "src").exists():
                crate_roots.append(p)

kw_unsafe = re.compile(r"\bunsafe\b")
kw_extern_c = re.compile(r"extern\s+\"C\"")
kw_unsafe_fn = re.compile(r"\bunsafe\s+fn\b")

crates = []
for c in sorted(crate_roots, key=lambda x: x.name):
    src = c / "src"
    unsafe_kw = 0
    extern_c = 0
    unsafe_fn = 0
    files = 0
    for rs in src.rglob("*.rs"):
        files += 1
        text = rs.read_text(encoding="utf-8", errors="ignore")
        unsafe_kw += len(kw_unsafe.findall(text))
        extern_c += len(kw_extern_c.findall(text))
        unsafe_fn += len(kw_unsafe_fn.findall(text))
    crates.append({
        "crate": c.name,
        "path": str(c.relative_to(root)),
        "files": files,
        "unsafe_kw": unsafe_kw,
        "unsafe_fn": unsafe_fn,
        "extern_c": extern_c,
    })

l0 = {"spark-core", "spark-uci"}
l0_unsafe_total = sum(item["unsafe_kw"] + item["extern_c"] + item["unsafe_fn"] for item in crates if item["crate"] in l0)

report = {
    "tool": "spark-unsafe-scan",
    "generated_at_unix": int(time.time()),
    "crates": crates,
    "summary": {
        "l0_crates": sorted(l0),
        "l0_unsafe_total": l0_unsafe_total,
    },
}

out_path.parent.mkdir(parents=True, exist_ok=True)
out_path.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

if l0_unsafe_total != 0:
    print(f"ERROR: L0 unsafe total is {l0_unsafe_total} (expected 0)")
    sys.exit(1)
print(f"Wrote {out_path}")
PY
