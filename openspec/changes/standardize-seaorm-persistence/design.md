# Design

## Context

See `proposal.md` for motivation and scope. `north-persistence` currently owns a SQLx `PgPool`, row/domain mappings, and 69 direct query sites across nine modules. Its current `0001_initial_schema.sql` is one consolidated baseline for 19 tables; it includes PostgreSQL checks, indexes, array/JSONB columns, and three immutability triggers. `north-server::build_app` applies SQLx migrations before `main` binds the listener. Release packaging copies the raw `migrations/` directory. `north-domain` must remain database-agnostic, and `north-persistence` remains the only production database boundary.

## Goals / Non-Goals

**Goals:**
- Make SeaORM the application-level connection, entity, transaction, and migration API.
- Preserve current PostgreSQL schema invariants and transaction/locking semantics.
- Move DDL to an explicit, packaged migration command; make ordinary startup read-only with respect to schema.
- Reject old SQLx-stamped/untracked pre-release databases before DDL; require manual recreation.

**Non-Goals:**
- Business/domain redesign, another database backend, automatic schema synchronization, or an upgrade path for unpublished SQLx databases.
- Replacing PostgreSQL-specific operations with less precise ORM expressions solely to eliminate SQL text.

## Decisions

1. **Keep one persistence component.** Add SeaORM 2.0.x and SeaORM migrations to `north-persistence`; define entity modules beside the persistence code. Replace `PgPool` with `DatabaseConnection` and SQLx row/error APIs with SeaORM types. Do not add a separate migration crate or move entities into `north-domain`.

2. **Use entities for schema and straightforward query paths.** Define entities for the 19 current tables and derive schema statements from those entities where the builder represents existing PostgreSQL types and relations. Use entity builders for the verified user list, lookup, and role-update paths. Keep other existing queries as bound SeaORM statements where an ORM rewrite could alter projections, lock order, returning values, or transaction boundaries. Do not introduce generic repositories or a second domain model.

3. **Keep exact PostgreSQL semantics for complex paths.** SeaORM `DatabaseConnection` and `DatabaseTransaction` own production database access. Multi-step daemon delivery, readiness, retry, advisory-lock, `FOR UPDATE SKIP LOCKED`, CTE, and conditional-upsert paths stay inside SeaORM transactions. Existing SQL uses bound SeaORM `Statement`s through one private persistence adapter; values remain parameters, not interpolated SQL. Migration builders own tables and supported constraints; localized PostgreSQL DDL stays in versioned Rust migrations for triggers, partial indexes, and unsupported checks. No runtime production code calls SQLx APIs directly.

4. **Replace raw migration assets with a Rust baseline.** The pre-release consolidation had left one final-schema SQL baseline. Replace it with a compiled SeaORM baseline under `north-persistence`; represent the final schema directly because old unpublished databases are not upgraded. Preserve the singleton settings seed, columns/defaults/FKs/checks/indexes/triggers, and application behavior. SeaORM emits PostgreSQL identity columns for former `BIGSERIAL` IDs; parity verifies attached sequences and generated-ID inserts. Explicit `Text` and `Text[]` entity types preserve those column types. Keep only DDL/data initialization required for an empty database; do not carry upgrade-only backfills into the new baseline.

5. **Apply pending migrations before serving under a shared lock.** Keep packaged `north-server migrate` and route it through the same guarded migration function as normal startup. Startup validates configuration, connects, then enters a PostgreSQL transaction-scoped advisory lock. Preflight, `Migrator::up`, and post-migration verification all use that transaction; commit must succeed before app construction or listener binding. Migration failure rolls back and releases the lock. The outer transaction makes the current migration batch atomic and serializes CLI/startup calls; current migrations use transaction-compatible PostgreSQL DDL. Do not expose `fresh`, `reset`, or `down` commands or enable entity-first schema sync. Future non-transactional DDL requires an explicit alternative that preserves cross-process locking.

6. **Fail closed on unsupported old schemas.** Before applying the baseline, reject an existing SQLx migration ledger or North application tables without the SeaORM ledger; return a safe manual-recreation message before changing application schema/data. Never infer that an existing database is empty, drop schemas, or auto-convert migration ledgers. Future published SeaORM migrations are append-only.

7. **Do not enable entity-first schema sync in production.** SeaORM's `schema-sync` discovers live schema and adds missing tables/columns/keys, but can drop indexes and runs as startup mutation. Use entity-derived schema statements only inside explicit, reviewed migrations; use versioned migrations for additive changes too.

8. **Update release workflow and tests.** The package contains the compiled migrator in `north-server`; remove runtime SQL migration assets from artifacts. Release qualification starts the packaged server against an empty isolated database and verifies that migration commits before traffic; it may qualify the explicit CLI separately. Integration tests connect and execute through SeaORM; the legacy SQL baseline remains a test-only catalog oracle, and PostgreSQL-specific catalog/constraint fixtures stay as bound SeaORM statements. Test baseline parity, idempotent `up`, automatic fresh-install startup, lock contention, legacy-database rejection without mutation, migration-failure startup refusal, and representative concurrent transaction paths.

## Risks / Trade-offs

- [Schema drift while translating 19 tables, checks, indexes, FKs, and three triggers] → Compare PostgreSQL catalog output; normalize only equivalent `BIGSERIAL`/identity and generated primary-key names, verify sequence-backed inserts, and run fresh-install/constraint/trigger integration tests before accepting the migration.
- [Replacing direct SQLx across 69 production query sites may alter locking or transaction semantics] → Keep complex SQL on one SeaORM transaction/connection and run existing concurrency, retry, dedupe, and readiness suites.
- [Old local databases cannot use the new migration ledger] → Detect before DDL, document manual recreation, and never reset automatically.
- [Local validation may lack PostgreSQL] → Integration/CI profiles require `NORTH_TEST_DATABASE_URL`; use an isolated disposable database and never treat compilation as migration proof.
- [Startup now requires schema DDL privileges] → Grant the configured server role the privileges needed by pending migrations; fail before app construction or binding when the role cannot apply them.
- [Concurrent startup and CLI migration attempts] → Route both through the same PostgreSQL transaction-scoped advisory lock; never bypass the shared migration function.
- [Future migration requires non-transactional DDL] → The current outer transaction makes the batch atomic; define a compatible lock/transaction strategy before adding such a migration.

## Migration Plan

1. Add SeaORM entities and a Rust baseline migration; compare generated PostgreSQL schema with current baseline.
2. Convert persistence APIs and test fixtures; remove direct SQLx use, SQLx migration macros, and root `.sql` migration files.
3. Add the shared guarded migration path and PostgreSQL transaction-scoped advisory lock; keep `build_app` side-effect-free and apply pending migrations before normal startup binds.
4. Update package/qualification scripts and operator docs. Qualify packaged startup against an empty database; keep `north-server migrate` as an optional entry point to the same path.
5. Existing pre-release SQLx databases are unsupported: operators stop server, preserve/export any needed data separately, then manually recreate database before first SeaORM migration. The command never drops or resets them.
6. After public release, apply only reviewed forward SeaORM migrations; rollback is a deployment/application rollback with a compatible schema, not an automatic migration reset.

## Open Questions

None. PostgreSQL remains the supported backend; schema sync stays disabled.
