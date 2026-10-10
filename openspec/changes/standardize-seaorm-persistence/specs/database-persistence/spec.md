# Spec Delta

## Purpose

Defines North's PostgreSQL persistence boundary, schema ownership, and safe migration lifecycle so database changes remain explicit, reviewed, and non-destructive.

## ADDED Requirements

### Requirement: Production persistence uses the SeaORM boundary

Production server persistence SHALL use SeaORM connections, entity models, and transaction APIs owned by the persistence component. Business-domain types SHALL remain independent of database and ORM types. PostgreSQL-specific operations SHALL use SeaORM statements or transactions without moving business-state ownership out of the server.

#### Scenario: Canonical persistence stays behind the ORM boundary

- **WHEN** a server operation reads or changes durable North state
- **THEN** it uses the SeaORM persistence boundary and preserves the existing PostgreSQL transaction, locking, constraint, and domain-transition semantics

### Requirement: Schema changes use guarded versioned migrations

The supported PostgreSQL schema SHALL be created and evolved through ordered, reviewed SeaORM migrations. Normal server startup SHALL run the shared migration path, apply pending checked-in migrations, verify the resulting head, and commit before binding its listener. The packaged `north-server migrate` command SHALL remain available and use that same path. Both entry points SHALL serialize migration work through the same PostgreSQL transaction-scoped advisory lock. Preflight, migration execution, and verification SHALL use the locked transaction. Entity-first schema synchronization, automatic reset, and schema dropping SHALL remain disabled. Additive changes, renames, backfills, and destructive changes SHALL use explicit versioned migrations.

#### Scenario: Fresh database is migrated before service

- **WHEN** the packaged server starts with valid configuration against an empty PostgreSQL schema
- **THEN** it applies the checked-in migrations, verifies and commits the migration head, and only then binds its listener without an external raw `.sql` script

#### Scenario: Current schema starts without schema changes

- **WHEN** the server starts against the current SeaORM migration head
- **THEN** it succeeds without changing application schema or data

#### Scenario: Startup without DDL privileges fails closed

- **WHEN** the server starts against an empty schema using a database role without schema `CREATE` privilege
- **THEN** it exits before listening, leaves the migration ledger and application tables absent, and reports a safe migration failure

#### Scenario: Concurrent migration entry points serialize

- **WHEN** startup and `north-server migrate` target the same database concurrently
- **THEN** one waits for the transaction-scoped advisory lock, observes the committed migration head after the first finishes, and does not race schema DDL

### Requirement: Existing pre-release SQLx schemas fail safely

The startup migration path and `north-server migrate` SHALL reject databases carrying the unsupported pre-release SQLx migration history or an existing North schema without the SeaORM baseline. They SHALL fail before changing application tables or data, identify the incompatibility without disclosing credentials, and instruct the operator to recreate the unpublished database manually. They SHALL NOT drop, reset, or reinterpret an existing database as fresh.

#### Scenario: SQLx migration history is present

- **WHEN** startup or the migration command targets a database stamped by the pre-release SQLx migrations
- **THEN** it exits unsuccessfully with manual-recreation guidance and leaves the schema and stored data unchanged

#### Scenario: Existing North schema has no SeaORM history

- **WHEN** startup or the migration command targets North application tables without a SeaORM migration ledger
- **THEN** it exits unsuccessfully before changing those tables or data and gives manual-recreation guidance

#### Scenario: Migration command is rerun on current schema

- **WHEN** the migration command targets a database already at the current SeaORM migration head
- **THEN** it succeeds without resetting the database or changing application tables or data
