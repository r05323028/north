# Tasks

## 1. Disposable PostgreSQL runner

- [x] 1.1 Add a validation-only workspace binary under `tests/pre-push-postgres/` using Testcontainers and PostgreSQL 16; construct the URL from discovered host/port, pass it to a child command, and unit-test URL construction. Verified with `cargo check --locked -p north-pre-push-postgres` and its URL unit test.
- [x] 1.2 Wait for the child, remove the tmpfs-backed container before returning its status, and verify success/failure propagation and cleanup. Probes covered child exit 37, Docker startup failure, readiness timeout, SIGINT (130), and SIGTERM (143); interruptions terminated child and removed container with no volume residue.

## 2. Pre-push integration and guidance

- [x] 2.1 Add the runner to the workspace and invoke its built binary from `scripts/pre-push-validation.sh` before Act; preserve direct/hosted database behavior and fail closed on Act platform skips unless native-only mode is explicit.
- [x] 2.2 Update `docs/development/ci.md` and `docs/development/testing.md`; pre-push ran with `NORTH_TEST_DATABASE_URL` unset and Docker available.

## 3. Validation

- [x] 3.1 Run `./scripts/validate.sh fast` and `openspec validate --all --strict`; both passed (34 OpenSpec items), including workspace format, clippy, architecture, and unit checks.
- [x] 3.2 With `NORTH_TEST_DATABASE_URL` unset, native `validate.sh ci` passed. Default pre-push failed closed when Act skipped `rust` (`self-hosted`); explicit `NORTH_PRE_PUSH_SKIP_ACT=1` passed native-only. Startup/readiness and child-exit probes passed; SIGINT/SIGTERM returned 130/143 and removed child/container, with no new volumes.
