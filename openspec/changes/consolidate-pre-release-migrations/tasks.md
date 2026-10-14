# Tasks

## 1. Establish one pre-release baseline

- [x] 1.1 Fold all unreleased schema into one compiled SeaORM baseline source `initial_schema.rs` (recorded ID `m0001_initial_schema`); remove numbered pre-release migration sources and registrations, stamp and preflight baseline provenance, and verify no database reset.
- [x] 1.2 Remove obsolete historical-upgrade test; make `fresh_install` assert the sole baseline head and complete current schema, and reject reused `m0001`, unregistered history, and empty-ledger partial schemas without mutation. Verify Rust compilation and isolated PostgreSQL tests against a disposable database.

## 2. Align contracts and operator guidance

- [x] 2.1 Update persistence, deployment, and testing docs to document baseline installation, manual recreation of pre-0.1.0 databases, no automatic reset, and append-only migrations after publication; align `docs/product/requirement-lifecycle.md` with the empty-schema baseline's lack of historical rank backfill and verify migration-history references agree.
- [x] 2.2 Revise only migration-specific claims in `prepare-0-1-0-release` artifacts and sync the OTP/retention spec deltas; verify `openspec validate --all --strict` passes.

## 3. Validate and review

- [x] 3.1 Run focused Rust/architecture checks and `./scripts/validate.sh fast`; run PostgreSQL fresh-install/integration tests only against an isolated database and record any unavailable prerequisites honestly.
> Validation note: `./scripts/validate.sh fast` passed; disposable PostgreSQL 16 tests passed for `fresh_install` (12/12) and `server_runtime` (2/2, including DDL-denied startup). The concurrent `requirements` target passed 10 consecutive runs, each with a private schema that was removed afterward. Strict OpenSpec passed 40/40, and `./scripts/pre-push-validation.sh` exited 0 (CI and Act Rust job). No matching on-disk `m[0-9]+_*.rs` files remain; no existing database or Compose service was touched. Task 3.2 remains open pending full-diff review.

- [ ] 3.2 Review complete diff including pre-existing worktree changes, run `./scripts/pre-push-validation.sh` because source/test/script files changed, and ensure no database reset or unrelated edits occurred.
