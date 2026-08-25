#!/usr/bin/env bash
# Pre-push gate: everything fast plus local GitHub Actions parity via act.
# Remote GitHub CI remains authoritative (docs/development/ci.md).
#
# Env knobs:
#   NORTH_PRE_PUSH_JOB       ci.yml job act runs (default: rust)
#   NORTH_PRE_PUSH_TIMEOUT   seconds per act invocation (default: 1800)
#   NORTH_PRE_PUSH_SKIP_ACT  set to 1 to skip act (documented escape hatch)
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"
JOB="${NORTH_PRE_PUSH_JOB:-rust}"
WORKFLOW=".github/workflows/ci.yml"
TIMEOUT="${NORTH_PRE_PUSH_TIMEOUT:-1800}"

step() { printf '\n==> %s\n' "$*"; }

step 'native merge-gate checks'
bash scripts/validate.sh ci

if [[ "${NORTH_PRE_PUSH_SKIP_ACT:-0}" == "1" ]]; then
  printf '\nact skipped (NORTH_PRE_PUSH_SKIP_ACT=1); GitHub CI remains authoritative.\n'
  exit 0
fi

command -v act >/dev/null || {
  printf 'act is required for pre-push CI parity.\nInstall: brew install act (macOS) / https://github.com/nektos/act\n' >&2
  printf 'Or push with NORTH_PRE_PUSH_SKIP_ACT=1 (documented exception; CI still gates the merge).\n' >&2
  exit 1
}
command -v docker >/dev/null || {
  printf 'docker is required (act runs workflow jobs in containers).\n' >&2
  exit 1
}
docker info >/dev/null 2>&1 || {
  printf 'Docker daemon not reachable — start Docker Desktop/colima and retry.\n' >&2
  exit 1
}
grep -qE "^[[:space:]]*${JOB}:" "$WORKFLOW" || {
  printf 'job %s not found in %s\nKnown jobs:\n' "$JOB" "$WORKFLOW" >&2
  grep -oE '^  [a-z0-9_-]+:' "$WORKFLOW" | tr -d ' :' | sed 's/^/  /' >&2
  exit 1
}

step "act parity: ${JOB} (${WORKFLOW})"
if command -v timeout >/dev/null 2>&1; then
  timeout "${TIMEOUT}" act -W "${WORKFLOW}" -j "${JOB}"
elif command -v gtimeout >/dev/null 2>&1; then
  gtimeout "${TIMEOUT}" act -W "${WORKFLOW}" -j "${JOB}"
else
  printf 'GNU timeout unavailable; running act without a local deadline.\n' >&2
  act -W "${WORKFLOW}" -j "${JOB}"
fi
printf '\npre-push validation complete.\n'
