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

run_act() {
  local output_file status
  output_file="$(mktemp)"
  set +e
  "$@" 2>&1 | tee "$output_file"
  status=${PIPESTATUS[0]}
  set -e
  if grep -qF 'Skipping unsupported platform' "$output_file"; then
    printf 'act skipped this job; workflow parity was not run. Set NORTH_PRE_PUSH_SKIP_ACT=1 for deliberate native-only validation, or configure an Act platform mapping.\n' >&2
    status=1
  fi
  rm -f "$output_file"
  return "$status"
}

step 'Docker preflight (Testcontainers PostgreSQL and Act)'
command -v docker >/dev/null || {
  printf 'docker is required for Testcontainers PostgreSQL and Act.\n' >&2
  exit 1
}
docker info >/dev/null 2>&1 || {
  printf 'Docker daemon not reachable — start Docker Desktop/colima and retry.\n' >&2
  exit 1
}

step 'build disposable PostgreSQL runner'
cargo build --locked --package north-pre-push-postgres
TARGET_DIR="$(cargo metadata --no-deps --format-version 1 | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')"
[[ -n "$TARGET_DIR" ]] || {
  printf 'could not determine Cargo target directory for PostgreSQL runner.\n' >&2
  exit 1
}
if [[ -n "${CARGO_BUILD_TARGET:-}" ]]; then
  POSTGRES_RUNNER="$TARGET_DIR/$CARGO_BUILD_TARGET/debug/north-pre-push-postgres"
else
  POSTGRES_RUNNER="$TARGET_DIR/debug/north-pre-push-postgres"
fi
[[ -x "$POSTGRES_RUNNER" ]] || {
  printf 'built PostgreSQL runner not found: %s\n' "$POSTGRES_RUNNER" >&2
  exit 1
}

step 'native merge-gate checks (disposable PostgreSQL 16)'
"$POSTGRES_RUNNER" -- bash scripts/validate.sh ci

if [[ "${NORTH_PRE_PUSH_SKIP_ACT:-0}" == "1" ]]; then
  printf '\nact skipped (NORTH_PRE_PUSH_SKIP_ACT=1); GitHub CI remains authoritative.\n'
  exit 0
fi

command -v act >/dev/null || {
  printf 'act is required for pre-push CI parity.\nInstall: brew install act (macOS) / https://github.com/nektos/act\n' >&2
  printf 'Or push with NORTH_PRE_PUSH_SKIP_ACT=1 (documented exception; CI still gates the merge).\n' >&2
  exit 1
}
grep -qE "^[[:space:]]*${JOB}:" "$WORKFLOW" || {
  printf 'job %s not found in %s\nKnown jobs:\n' "$JOB" "$WORKFLOW" >&2
  grep -oE '^  [a-z0-9_-]+:' "$WORKFLOW" | tr -d ' :' | sed 's/^/  /' >&2
  exit 1
}

step "act parity: ${JOB} (${WORKFLOW})"
if command -v timeout >/dev/null 2>&1; then
  run_act timeout "${TIMEOUT}" act -W "${WORKFLOW}" -j "${JOB}"
elif command -v gtimeout >/dev/null 2>&1; then
  run_act gtimeout "${TIMEOUT}" act -W "${WORKFLOW}" -j "${JOB}"
else
  printf 'GNU timeout unavailable; running act without a local deadline.\n' >&2
  run_act act -W "${WORKFLOW}" -j "${JOB}"
fi
printf '\npre-push validation complete.\n'
