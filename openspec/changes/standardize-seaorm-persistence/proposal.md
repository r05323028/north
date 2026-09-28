# Proposal

## Why

North's persistence crate currently exposes SQLx directly and maintains handwritten SQL queries across nine modules; its single raw-SQL baseline is also applied inside normal server startup. SeaORM can provide one typed persistence boundary and Rust-versioned migration system while keeping schema changes explicit, reviewed, and non-destructive.

## What Changes

- Replace direct SQLx persistence APIs with SeaORM `DatabaseConnection`, entities, and transactions inside `north-persistence`; keep domain types and business ownership unchanged.
- Use entity CRUD for straightforward operations and SeaORM statements for PostgreSQL-specific locking, atomic writes, triggers, and other cases where typed operations would risk changing semantics.
- Replace raw `.sql` migration files with versioned Rust SeaORM migrations. Use SeaORM/SeaQuery schema builders for entity-backed tables and supported constraints/indexes; isolate remaining PostgreSQL-specific DDL in explicit migrations.
- Add an explicit `north-server migrate` operation. Normal startup SHALL NOT apply migrations or run schema sync; it SHALL fail clearly when the schema is absent or behind.
- Reject pre-release databases carrying SQLx migration history or an existing incompatible schema with manual-recreation guidance. Never auto-reset or silently treat them as fresh.
- Evaluate SeaORM entity-first schema sync and keep it disabled in production startup because it performs live schema discovery and may drop indexes.
- Update integration/release tests, release packaging, operator documentation, and dependency boundaries; remove direct SQLx code and raw migration artifacts once SeaORM fully replaces them.

## Capabilities

### New Capabilities
- `database-persistence`: SeaORM ownership, entity-based persistence, explicit versioned migrations, and safe initialization.

### Modified Capabilities
- `server-runtime`: migration moves from startup to an explicit command; startup verifies schema readiness without changing schema.
- `release-qualification`: qualification explicitly migrates an isolated database before starting the packaged server.

## Impact

Affected: `crates/north-persistence`, `crates/north-server`, Cargo manifests/lockfile, Rust integration tests, `migrations/`, release packaging/qualification scripts, and persistence/deployment/testing docs. PostgreSQL remains the supported database. No domain, HTTP, protocol, or product lifecycle redesign.
