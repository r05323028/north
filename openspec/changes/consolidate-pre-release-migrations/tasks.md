# Tasks

## 1. Establish one pre-release baseline

- [x] 1.1 Combine migration SQL in original version order into `0001_initial_schema.sql`, remove the old numbered files, and update `migrations/README.md`; verify only one migration is embedded and no database is reset.
- [ ] 1.2 Remove obsolete historical-upgrade test and integration invocation; make `fresh_install` assert exactly the baseline head and current schema. Verify Rust compilation and the isolated fresh-install test when `NORTH_TEST_DATABASE_URL` is available.

## 2. Align contracts and operator guidance

- [x] 2.1 Update persistence, deployment, and testing docs to document baseline installation, manual recreation of pre-0.1.0 databases, no automatic reset, and append-only migrations after publication; verify all migration-history references agree.
- [x] 2.2 Revise only migration-specific claims in `prepare-0-1-0-release` artifacts and sync the OTP/retention spec deltas; verify `openspec validate --all --strict` passes.

## 3. Validate and review

- [ ] 3.1 Run focused Rust/architecture checks and `./scripts/validate.sh fast`; run PostgreSQL fresh-install/integration tests only against an isolated database and record any unavailable prerequisites honestly.
> Validation note: `./scripts/validate.sh fast` passed. `NORTH_TEST_DATABASE_URL` was unset; pre-push stopped with `validate.sh: integration requires NORTH_TEST_DATABASE_URL.` The PostgreSQL fresh-install test was not executed.

- [ ] 3.2 Review complete diff including pre-existing worktree changes, run `./scripts/pre-push-validation.sh` because source/test/script files changed, and ensure no database reset or unrelated edits occurred.
