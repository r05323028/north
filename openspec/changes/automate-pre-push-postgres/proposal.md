# Proposal: Automatically provision PostgreSQL for pre-push

## Why

Local pre-push currently stops when `NORTH_TEST_DATABASE_URL` is unset, before PostgreSQL-backed integration tests or Act parity run. Developers should not need to keep or manually configure a database for one validation run.

## What Changes

- Add a Testcontainers-backed validation runner that starts disposable PostgreSQL 16 for local pre-push.
- Pass the container’s dynamically mapped connection URL to the existing `./scripts/validate.sh ci` child process, then clean up the container and preserve the child exit status.
- Keep direct `validate.sh ci` and `integration` invocations explicit; hosted CI continues using its existing PostgreSQL service.
- Fail pre-push when Act returns success after skipping the selected job; keep the explicit native-only skip flag.
- Document Docker as a pre-push prerequisite and remove the manual local database setup step.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `development-harness`: local pre-push automatically provisions disposable PostgreSQL before running the shared native CI profile, then retains Act parity.

## Dependencies

No earlier change required. Existing PostgreSQL integration tests and the shared `validate.sh ci` profile remain the validation source of truth.

## Impact

- `scripts/pre-push-validation.sh`, a validation-only runner under `tests/`, and Cargo workspace/lock metadata.
- `docs/development/ci.md` and `docs/development/testing.md` document automatic local provisioning and the remaining explicit URL contract for direct profiles.
- Adds Testcontainers and signal-handling dependencies only to validation tooling; no production crate, application runtime, migration, or hosted CI service changes.
