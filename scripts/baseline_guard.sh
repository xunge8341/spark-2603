#!/usr/bin/env bash
set -euo pipefail

# Guardrail: baseline changes must be accompanied by a decision log entry.
#
# Policy:
# - If any file under perf/baselines/ changes, docs/DECISION_LOG.md must also change.
# - Intended to run in CI for pull requests (github.event.pull_request).

if [[ -z "${GITHUB_BASE_REF:-}" ]]; then
  echo "baseline_guard: no GITHUB_BASE_REF detected; skipping." >&2
  exit 0
fi

base_ref="origin/${GITHUB_BASE_REF}"

git fetch --no-tags --depth=1 origin "${GITHUB_BASE_REF}" >/dev/null

changed=$(git diff --name-only "${base_ref}...HEAD" || true)

if [[ -z "$changed" ]]; then
  echo "baseline_guard: no changes detected." >&2
  exit 0
fi

baseline_changed=$(echo "$changed" | grep -E '^perf/baselines/' || true)
decision_changed=$(echo "$changed" | grep -E '^docs/DECISION_LOG\.md$' || true)

if [[ -n "$baseline_changed" && -z "$decision_changed" ]]; then
  echo "ERROR: perf baselines changed without updating docs/DECISION_LOG.md" >&2
  echo "Changed baseline files:" >&2
  echo "$baseline_changed" | sed 's/^/  - /' >&2
  echo "\nExpected: docs/DECISION_LOG.md includes a rationale for baseline updates." >&2
  exit 1
fi

echo "OK: baseline_guard passed." >&2
