# Spec Delta

## Purpose

Defines North's PostgreSQL persistence boundary, schema ownership, and safe migration lifecycle so database changes remain explicit and application data is never silently reset.

## ADDED Requirements

### Requirement: Production persistence uses the SeaORM boundary

Production server persistence SHALL use SeaORM connections, entity models, and transaction APIs owned by the persistence component. Business-domain types SHALL remain independent of database and ORM types. PostgreSQL-specific operations SHALL use SeaORM statements or transactions without moving business-state ownership out of the server.

#### Scenario: Canonical persistence stays behind the ORM boundary

- **WHEN** a server operation reads or changes durable North state
- **THEN** it uses the SeaORM persistence boundary and preserves the existing PostgreSQL transaction, locking, constraint, and domain-transition semantics

### Requirement: Schema changes use explicit versioned migrations

The supported PostgreSQL schema SHALL be created and evolved through ordered, versioned SeaORM migrations invoked by the explicit migration command. Deployment tooling SHALL serialize migration-command invocations per database and start or replace server processes only after successful migration completion. Normal server startup and request handling SHALL NOT apply migrations, synchronize entities, or otherwise change database schema. Entity-first schema synchronization SHALL NOT be enabled in production startup. Additive changes SHALL still be reviewed migrations; renames, backfills, and destructive changes SHALL use explicit versioned migrations.

#### Scenario: Fresh database receives the versioned baseline

- **WHEN** an operator runs the migration command against an empty PostgreSQL database
- **THEN** the SeaORM baseline creates the supported schema in migration order and records its migration head without using an external raw `.sql` script

#### Scenario: Ordinary startup does not alter schema

- **WHEN** the server starts against a database with the current SeaORM migration head
- **THEN** it verifies schema readiness without running schema synchronization or DDL

#### Scenario: Pending startup works without DDL privileges

- **WHEN** the server starts against an empty schema using a database role without schema `CREATE` privilege
- **THEN** it reports pending migrations without creating the migration ledger or application tables and never binds its listener

### Requirement: Existing pre-release SQLx schemas fail safely

The migration command SHALL reject databases carrying the unsupported pre-release SQLx migration history or an existing North schema without the SeaORM baseline. It SHALL fail before changing application tables or data, identify the incompatibility without disclosing credentials, and instruct the operator to recreate the unpublished database manually. It SHALL NOT drop, reset, or reinterpret an existing database as fresh.

#### Scenario: SQLx migration history is present

- **WHEN** the migration command targets a database stamped by the pre-release SQLx migrations
- **THEN** it exits unsuccessfully with manual-recreation guidance and leaves the schema and stored data unchanged

#### Scenario: Migration command is rerun on current schema

- **WHEN** the migration command targets a database already at the current SeaORM migration head
- **THEN** it succeeds without resetting the database or changing application data
