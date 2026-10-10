# Proposal

## Why

North's persistence crate currently exposes SQLx directly and maintains handwritten SQL queries across nine modules; its single raw-SQL baseline is also applied inside normal server startup. SeaORM can provide one typed persistence boundary and Rust-versioned migration system while keeping schema changes reviewed, versioned, and non-destructive.

## What Changes

- Replace direct SQLx persistence APIs with SeaORM `DatabaseConnection`, entities, and transactions inside `north-persistence`; keep domain types and business ownership unchanged.
- Use entities for schema and straightforward operations; retain bound SeaORM statements for PostgreSQL locking, atomic writes, triggers, and paths where typed operations risk changing semantics.
- Replace raw `.sql` migration files with reviewed, versioned Rust SeaORM migrations; keep entity-first schema sync disabled in production.
- Apply pending guarded migrations automatically during normal server startup before app construction or listener binding. Keep packaged `north-server migrate` as an optional operator command; both entry points use the same PostgreSQL transaction-scoped advisory lock.
- Reject pre-release SQLx history or unknown partial schemas before mutation. Never auto-reset, drop, or reinterpret an existing database as fresh.
- Update integration/release tests, release packaging, operator documentation, and dependency boundaries; remove direct SQLx code and raw migration artifacts once SeaORM fully replaces them.

## Capabilities

### New Capabilities
- `database-persistence`: SeaORM ownership, safe versioned migrations, guarded startup application, and an explicit CLI path.

### Modified Capabilities
- `server-runtime`: startup applies pending versioned migrations and commits before binding.
- `release-qualification`: qualification proves packaged startup can migrate an empty isolated database before serving.

## Impact

Affected: `crates/north-persistence`, `crates/north-server`, Cargo manifests/lockfile, Rust integration tests, `migrations/`, release packaging/qualification scripts, and persistence/deployment/testing docs. PostgreSQL remains the supported database. No domain, HTTP, protocol, or product lifecycle redesign.
