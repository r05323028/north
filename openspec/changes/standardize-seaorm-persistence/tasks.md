# Tasks

## 1. SeaORM dependencies and entities

- [x] 1.1 Add pinned workspace dependencies for SeaORM and sea-orm-migration with PostgreSQL/Tokio features; verify dependency resolution and `cargo check --workspace`.
- [x] 1.2 Define SeaORM entities for every current application table without leaking ORM types into `north-domain`; verify entity table/column mappings compile and match the current baseline.
- [x] 1.3 Run `cargo test -p north-persistence` and `./scripts/validate.sh fast`; resolve entity and architecture-boundary failures before migration work.

## 2. Rust migration baseline and safety

- [x] 2.1 Replace the raw SQL baseline with a compiled SeaORM baseline migration preserving all columns, types, defaults, keys, checks, indexes, sequences, triggers, and seed data; verify fresh-database catalog parity against the former baseline with PostgreSQL.
- [x] 2.2 Add migration integration tests for fresh install, repeat `up`, SQLx ledger rejection, existing/partial unknown-schema rejection, and unchanged schema/data on preflight failure; run with `NORTH_TEST_DATABASE_URL`.
- [x] 2.3 Verify migration errors redact credentials and migration success leaves application rows unchanged on an already-current database; run targeted PostgreSQL migration tests.

## 3. Convert persistence and test database access

- [x] 3.1 Replace direct SQLx persistence APIs across all modules with SeaORM connections, entities, statements, and transactions; verify existing persistence unit and PostgreSQL integration tests preserve locking, retry, dedupe, and domain behavior.
- [x] 3.2 Convert integration-test database setup and fixtures to the SeaORM boundary, retaining raw statements only for PostgreSQL-specific SQL/constraint fixtures; verify no direct SQLx database access remains outside `north-persistence`.
- [x] 3.3 Run `cargo test -p north-persistence` and affected `north-server` PostgreSQL integration tests with `NORTH_TEST_DATABASE_URL`; verify architecture checks still enforce the persistence boundary.

## 4. Guarded startup migrations and CLI

- [x] 4.1 Add packaged `north-server migrate` using the SeaORM migrator, with legacy/unknown-schema preflight and no listener; verify command behavior for fresh, current, and rejected databases using PostgreSQL integration tests.
- [x] 4.2 Apply pending guarded versioned migrations during normal process startup before app construction or listener binding; keep `build_app` side-effect-free and verify fresh-database startup, current-head no-op through the shared runner/CLI, DDL-privilege failure, and no listener on migration failure.
- [x] 4.3 Verify `north-server migrate` does not require server-only OTP configuration, never starts HTTP, and reports success/failure without credentials; run command-level tests.
- [x] 4.4 Serialize startup and CLI migration attempts with one PostgreSQL transaction-scoped advisory lock. Run preflight, migrations, and verification through the locked transaction; verify lock contention, commit-before-listen, and rollback/fail-closed behavior with PostgreSQL integration tests.

## 5. Packaging, docs, and obsolete-code removal

- [x] 5.1 Embed compiled migrations in the packaged server and remove raw SQL migration assets from release packaging; verify the packaged binary can migrate an isolated database when run outside the checkout.
- [x] 5.2 Update deployment, persistence, testing, invariant, and release-qualification docs to describe automatic startup migrations, required DDL privilege, shared lock, and optional explicit CLI; sync the existing server-runtime delta into canonical `openspec/specs/server-runtime/spec.md` and verify startup, no-op, failure, and optional CLI semantics agree.
- [x] 5.3 Remove obsolete SQLx migration macros, raw migration files, and direct SQLx dependencies where no production/test code still uses them; verify dependency/reference searches show SeaORM as the only application database API.
- [ ] 5.4 Run `./scripts/validate.sh fast`, architecture checks, strict OpenSpec validation, packaged release verification, `./scripts/validate.sh ci` with `NORTH_TEST_DATABASE_URL`, and required pre-push validation; record unavailable PostgreSQL/release evidence as incomplete rather than passing.

Validation note: `./scripts/validate.sh fast`, `./scripts/validate.sh ci` with disposable PostgreSQL 16, strict OpenSpec, PostgreSQL startup/lock integration tests, and pre-push Act Rust parity passed after automatic startup migration was implemented. Exact packaged-release auto-migration qualification remains unrun; task 5.4 stays pending.
