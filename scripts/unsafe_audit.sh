#!/usr/bin/env bash
set -euo pipefail

die(){ echo "ERROR: $*" 1>&2; exit 1; }

registry="docs/UNSAFE_REGISTRY.md"
[ -f "$registry" ] || die "missing $registry"

unsafe_pat='(^|[^[:alnum:]_])unsafe[[:space:]]*(\{|fn)'

scan_doc() {
  local path="$1"
  awk -v file="$path" '
    BEGIN { N=8; for (i=0;i<N;i++) buf[i]=""; idx=0 }
    function has_safety() {
      for (k=1;k<=N;k++) {
        j = idx - k;
        while (j < 0) j += N;
        if (buf[j] ~ /\/\/[^\n]*SAFETY:/) return 1;
      }
      return 0;
    }
    {
      line=$0
      buf[idx]=line
      idx=(idx+1)%N
      if (line ~ /(^|[^[:alnum:]_])unsafe[[:space:]]*(\{|fn)/) {
        if (!has_safety()) {
          printf("UNDOCUMENTED UNSAFE: %s:%d: %s\n", file, NR, line) > "/dev/stderr"
          exit 1
        }
      }
    }
  ' "$path"
}

mapfile -t unsafe_files < <(rg -n "$unsafe_pat" crates -g '*.rs' --no-heading | cut -d: -f1 | sort -u)

for f in "${unsafe_files[@]}"; do
  scan_doc "$f"
  if ! rg -n "^## ${f}$" "$registry" >/dev/null; then
    die "unsafe registry missing file entry: $f"
  fi
done

mapfile -t registry_files < <(awk '/^## crates\// {print $2}' "$registry" | sort -u)
for f in "${registry_files[@]}"; do
  if [ ! -f "$f" ]; then
    die "unsafe registry references missing file: $f"
  fi
  if ! printf '%s\n' "${unsafe_files[@]}" | rg -x "$f" >/dev/null; then
    die "unsafe registry stale entry (no unsafe in file): $f"
  fi
done

echo "OK: unsafe audit passed (documented + registry-synced)."
