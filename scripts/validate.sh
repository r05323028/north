#!/usr/bin/env bash
# Unified validation entrypoint (docs/development/testing.md defines layers).
# Usage: ./scripts/validate.sh [fast|unit|integration|e2e|smoke|ci]
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
  cargo test --workspace --lib  # Unit layer (north-domain et al.)
  cargo test -p north-archtests # Architecture checks
}

web_lint_tc() {
  (cd apps/web && npm run lint && npm run typecheck)
}

rust_full() {
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
}

case "$PROFILE" in
fast)
  rust_fast
  web_lint_tc
  openspec validate --all --strict
  ;;
unit)
  # Unit layer: small units, minimal externals. Frontend unit tests do not
  # exist yet; they arrive with introduce-requirement-board.
  cargo test --workspace --lib
  cargo test -p north-archtests
  printf '(frontend unit layer: not yet implemented — see testing.md)\n'
  ;;
integration)
  # Real component-boundary tests (server+persistence+DB, host-git workspace,
  # protocol round-trips). Arrives with introduce-email-auth-and-owner-bootstrap.
  unsupported
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
  # Complete merge gate mirror: full workspace tests plus production build.
  rust_full
  web_lint_tc
  (cd apps/web && npm run build)
  openspec validate --all --strict
  ;;
*)
  printf 'unknown profile: %s\nusage: %s [fast|unit|integration|e2e|smoke|ci]\n' "$PROFILE" "$0" >&2
  exit 2
  ;;
esac
printf 'validate.sh %s: OK\n' "$PROFILE"
