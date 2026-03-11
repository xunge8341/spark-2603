#!/usr/bin/env bash
set -euo pipefail

die(){ echo "ERROR: $*" 1>&2; exit 1; }

# Industrial-grade policy:
# 1) In core `spark-transport`, unsafe must be confined to a small, auditable set of modules.
# 2) Every `unsafe {}` block / `unsafe fn` item must be documented with a nearby `// SAFETY:` (or `// Safety:`) comment.
# 3) Leaf backends (e.g., IOCP) may use unsafe, but still must document it.

transport_root="crates/spark-transport/src"
allow_transport_unsafe=(
  "crates/spark-transport/src/lease.rs"
  "crates/spark-transport/src/reactor/event_buf.rs"
  "crates/spark-transport/src/async_bridge/channel_state.rs"
)

iocp_root="crates/spark-transport-iocp/src"

# Note: we avoid grep's non-portable word-boundary escapes and instead use a conservative delimiter.
unsafe_pat='(^|[^[:alnum:]_])unsafe[[:space:]]*(\{|fn)'

scan_doc() {
  local path="$1"
  awk -v file="$path" '
    BEGIN { N=8; for (i=0;i<N;i++) buf[i]=""; idx=0 }
    function has_safety() {
      for (k=1;k<=N;k++) {
        j = idx - k;
        while (j < 0) j += N;
        if (buf[j] ~ /\/\/[^\n]*(SAFETY|Safety):/) return 1;
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

# 1) Confinement: spark-transport must not grow unsafe surface area.
unsafe_hits=$(grep -R --line-number -E "$unsafe_pat" "$transport_root" || true)
if [ -n "$unsafe_hits" ]; then
  bad="$unsafe_hits"
  for ok in "${allow_transport_unsafe[@]}"; do
    bad=$(echo "$bad" | grep -v -E "^${ok}:" || true)
  done
  if [ -n "$bad" ]; then
    echo "$bad" | sed 's/^/UNSAFE FORBIDDEN: /'
    die "unsafe must be confined to the audited transport modules (lease/event_buf/channel_state)"
  fi
fi

# 2) Documentation: enforce SAFETY comments for the allowed unsafe modules.
for f in "${allow_transport_unsafe[@]}"; do
  [ -f "$f" ] || continue
  scan_doc "$f"
done

# 3) Leaf IOCP backend: allow unsafe but require documentation.
if [ -d "$iocp_root" ]; then
  while IFS= read -r -d '' f; do
    scan_doc "$f"
  done < <(find "$iocp_root" -name '*.rs' -print0)
fi

echo "OK: unsafe audit passed (confined + documented)."
