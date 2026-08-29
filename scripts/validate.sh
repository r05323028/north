#!/usr/bin/env bash
# Unified validation entrypoint (docs/development/testing.md defines layers).
# Usage: ./scripts/validate.sh [fast|rust|web|specs|unit|integration|e2e|smoke|ci]
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"
PROFILE="${1:-fast}"

unsupported() {
  printf 'validate.sh: profile "%s" has no executable tests yet.\n' "$PROFILE" >&2
  printf 'Its contract is defined in docs/development/testing.md.\n' >&2
  printf 'Enable it when real tests exist — do not fake a suite for the name.\n' >&2
  exit 3
}

rust_fast() {
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace --lib           # Unit layer (north-domain et al.)
  cargo test -p north-architecture-tests # Architecture checks
}

web_lint_tc() {
  (cd apps/web && npm run lint && npm run typecheck && npm run check:repository-settings)
}

web_full() {
  (cd apps/web && npm run lint && npm run typecheck && npm run check:repository-settings && npm run build)
}

rust_full() {
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
}

database_integration() {
  # Real component-boundary tests. PostgreSQL URL is an explicit prerequisite.
  if [[ -z "${NORTH_TEST_DATABASE_URL:-}" ]]; then
    printf 'validate.sh: integration requires NORTH_TEST_DATABASE_URL.\n' >&2
    exit 2
  fi
  if [[ "${1:-run-persistence}" != "skip-persistence" ]]; then
    cargo test -p north-persistence --all-targets
  fi
  cargo test -p north-server --test requirements -- --ignored
  cargo test -p north-server --test conversations_readiness -- --ignored
  cargo test -p north-server --test daemon_runtime -- --ignored
  cargo test -p north-server --test migration_upgrade -- --ignored
  cargo test -p north-server --test repositories -- --ignored
  cargo test -p north-server --test protocol_delivery -- --ignored
  cargo test -p north-transport-integration --test websocket
}

case "$PROFILE" in
fast)
  rust_fast
  web_lint_tc
  openspec validate --all --strict
  ;;
rust)
  rust_full
  ;;
web)
  web_full
  ;;
specs)
  openspec validate --all --strict
  ;;
unit)
  # Unit layer: small units, minimal externals. Frontend unit tests do not
  # exist yet; they arrive with introduce-requirement-board.
  cargo test --workspace --lib
  cargo test -p north-architecture-tests
  printf '(frontend unit layer: not yet implemented — see testing.md)\n'
  ;;
integration)
  database_integration
  ;;
e2e)
  # Full user-workflow tests across assembled North (Playwright when UI lands).
  unsupported
  ;;
smoke)
  # Start-and-serve probes (server boots, migrations apply, health responds).
  unsupported
  ;;
ci)
  # Complete merge gate mirror: full workspace and database tests plus build.
  rust_full
  database_integration skip-persistence
  web_full
  openspec validate --all --strict
  ;;
*)
  printf 'unknown profile: %s\nusage: %s [fast|rust|web|specs|unit|integration|e2e|smoke|ci]\n' "$PROFILE" "$0" >&2
  exit 2
  ;;
esac
printf 'validate.sh %s: OK\n' "$PROFILE"
