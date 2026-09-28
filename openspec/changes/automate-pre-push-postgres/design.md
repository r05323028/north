# Design

## Context

See `proposal.md` for motivation and `specs/development-harness/spec.md` for behavior. The existing CI profile deliberately accepts an explicit database URL; hosted CI supplies its own PostgreSQL service. Local pre-push runs that profile before Act.

## Goals / Non-Goals

**Goals:**

- Provision one PostgreSQL 16 instance for all database-backed commands in one local pre-push run.
- Reuse `validate.sh ci` and pass the mapped URL only to that child process.
- Keep the helper outside production `crates/` and remove its container before native validation returns.

**Non-Goals:**

- Start Testcontainers from `validate.sh ci` or alter hosted CI’s service database.
- Rewrite integration tests to own separate containers.
- Keep a persistent container, volume, fixed host port, or database URL configuration.

## Decisions

1. **One Testcontainers Rust runner under `tests/pre-push-postgres/`.** Existing integration suites run as multiple Cargo processes and already consume one `NORTH_TEST_DATABASE_URL`. Start one container around the existing `validate.sh ci` child instead of refactoring every suite or adding a parallel validation path. A workspace test-only crate respects the `crates/` production boundary.
2. **PostgreSQL 16, dynamic port, Testcontainers-reported host.** Match hosted service major version, expose 5432 on an automatically selected host port, and build the URL from Testcontainers’ host/port discovery. Do not assume `127.0.0.1`, so supported remote Docker contexts can provide their reachable host. Mount PostgreSQL data directory on tmpfs; the upstream image declares a volume, and tmpfs prevents anonymous-volume residue.
3. **Build helper, then execute its binary.** Pre-push builds the runner before invoking it; the runner then starts `bash scripts/validate.sh ci` as a child. This avoids invoking Cargo from a process launched by `cargo run`, preserves the normal validation entrypoint, and scopes the URL to the child environment.
4. **Remove before returning child status.** Wait for native CI to exit, call Testcontainers’ synchronous `rm()`, then return the child status. On SIGINT/SIGTERM, terminate the child, remove the container, and return `128 + signal`. On cleanup failure after child success, fail; after child failure, log cleanup failure and preserve the child status. Do not call `std::process::exit`. Startup or endpoint-discovery errors fail before CI/Act.
5. **Hosted and direct profiles remain explicit.** Only `pre-push-validation.sh` uses the runner. `validate.sh ci` and `integration` keep their current URL prerequisite for hosted services and direct invocations.
6. **An Act skip is not parity.** Act can exit successfully after skipping a `self-hosted` job. Capture its output and fail when the selected job is skipped; `NORTH_PRE_PUSH_SKIP_ACT=1` remains the explicit native-only escape hatch.

## Risks / Trade-offs

- Docker is unavailable or image pull/start fails → fail clearly before native CI and Act; do not fall back to a manually configured URL.
- Integration tests mutate database contents → use one fresh container with tmpfs-backed data and test-only credentials; no named or anonymous volume persists.
- Remote Docker networking differs from local Docker Desktop → use Testcontainers host discovery plus the dynamic port; the PostgreSQL suites verify the connection from the child process.
- SIGKILL or host loss bypasses signal handlers and can leave a running tmpfs-only container → add a daemon-level reaper only if hard-crash cleanup must be guaranteed. SIGINT/SIGTERM and normal child exits remove it synchronously.

## Migration Plan

No data migration. Build the validation runner and switch only the local pre-push native-CI step to execute it. Hosted workflows and direct `validate.sh ci` remain unchanged. Rollback restores the original direct `bash scripts/validate.sh ci` invocation and removes the helper workspace member/dependency.
