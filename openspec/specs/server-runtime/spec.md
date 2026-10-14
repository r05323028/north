# server-runtime Specification


## Purpose

Defines externally visible behavior of the supported North server process used by the 0.1.0 self-hosted release. It keeps startup, migration, health, and failure behavior in one server-owned contract so release qualification does not rely on an in-process test router.

## Requirements

### Requirement: Server starts only after mandatory configuration and migrations succeed

The server SHALL validate configuration and current migration state before listening; schema changes SHALL be made only by checked-in, versioned migrations through the guarded persistence path.



#### Scenario: Valid production-shaped startup

- **WHEN** the process receives a reachable PostgreSQL URL at an empty, pending, or current supported schema, a valid OTP HMAC key, and an optional bind address
- **THEN** it validates configuration, applies and verifies pending migrations through the guarded path, commits before binding the requested address, and serves the existing authenticated HTTP routes
- **AND** the complete requirement-level contract applies:
  The North server process SHALL require `DATABASE_URL` and a valid 64-character hexadecimal `NORTH_OTP_HMAC_KEY`. It MAY accept `NORTH_BIND_ADDR`; when absent it SHALL bind its documented loopback default. Before accepting HTTP traffic, it SHALL validate configuration, connect to PostgreSQL, and verify that all checked-in SeaORM migrations are applied. Normal server startup SHALL apply only checked-in pending versioned migrations through the guarded path and verify the resulting migration head. Startup and CLI migration entry points SHALL serialize through one PostgreSQL transaction-scoped advisory lock, and startup SHALL commit migration and verification before app construction and listener bind. When no migrations are pending, it SHALL leave application schema and data unchanged. The server database role SHALL have the DDL privileges required by pending migrations. Entity-first schema synchronization, automatic reset, and schema drops SHALL NOT occur. It SHALL use the same server router and domain ownership rules as library/integration execution; it SHALL NOT expose an unkeyed OTP path or a second business-state implementation.

#### Scenario: Missing or malformed OTP key fails closed

- **WHEN** `NORTH_OTP_HMAC_KEY` is missing, empty, non-hexadecimal, or not exactly 64 hexadecimal characters
- **THEN** the process exits non-successfully before accepting HTTP traffic, identifies invalid configuration without printing the key, and never issues or verifies one-time codes through an unkeyed path

#### Scenario: Database or migration failure fails before listen

- **WHEN** `DATABASE_URL` is missing, PostgreSQL is unavailable, migration preflight rejects the schema, migration execution or commit fails, the database role lacks required DDL privileges, or database schema provenance/state is incompatible
- **THEN** the process exits non-successfully before serving authenticated routes, reports the failure class without database credentials, directs operators to the documented recovery path when appropriate, and leaves no partially started listener

### Requirement: Server exposes an explicit migration command

The packaged server MAY expose `north-server migrate` as an optional operator command. It SHALL apply only pending checked-in versioned SeaORM migrations through the same guarded migration path and advisory lock as startup, SHALL NOT expose an automatic reset or schema-sync operation, and SHALL exit non-successfully without changing application data when migration preflight or migration execution fails.

#### Scenario: Operator applies pending migrations explicitly

- **WHEN** an operator runs `north-server migrate` with a reachable `DATABASE_URL`
- **THEN** the command applies pending SeaORM migrations in order, reports success, and exits without starting the HTTP listener

#### Scenario: Up-to-date schema remains unchanged

- **WHEN** an operator runs `north-server migrate` against a database already at the current migration head
- **THEN** the command succeeds without resetting or changing application tables or data

### Requirement: Server exposes safe operational readiness

GET /healthz SHALL be a non-secret process/liveness signal after startup, not a continuous database-readiness claim.



#### Scenario: Health is unavailable before startup completes

- **WHEN** configuration, database connection, or schema-state validation is still pending or has failed
- **THEN** `GET /healthz` is not served as successful and the process does not accept normal authenticated traffic

#### Scenario: Database loss after startup does not change health semantics

- **WHEN** PostgreSQL becomes unavailable after a successful startup
- **THEN** `/healthz` remains a process/liveness signal rather than claiming continuous database readiness, authenticated requests use existing database-error behavior, and an unrecoverable process exits non-successfully

#### Scenario: Healthy process returns safe response

- **WHEN** startup completes successfully against a supported schema after applying and verifying any pending migrations
- **THEN** `GET /healthz` returns a successful, non-secret response suitable for a reverse-proxy probe and authenticated routes remain governed by existing session and role checks
- **AND** the complete requirement-level contract applies:
  After mandatory startup completes, the server SHALL expose an unauthenticated `GET /healthz` endpoint for process/liveness checks. A successful response SHALL be non-secret and SHALL mean that configuration and database connection succeeded and the current migration head was verified and committed for that process; any pending migrations were applied through the guarded path before listener bind, but it SHALL NOT promise continuous PostgreSQL availability. The endpoint SHALL NOT expose OTP keys, database credentials, session cookies, daemon credentials, verification codes, or arbitrary database contents.


### Requirement: Server lifetime is not limited by the shutdown grace period

The server SHALL continue accepting requests after startup until it receives a termination signal or encounters a serve failure. `SHUTDOWN_TIMEOUT` SHALL bound only the graceful drain after `SIGINT` or `SIGTERM`; it SHALL NOT terminate a healthy server during normal operation. When the drain exceeds the bound, the process SHALL exit non-successfully without secret-bearing output.

#### Scenario: Server remains live beyond shutdown grace period

- **GIVEN** the server started with valid configuration and a current schema
- **WHEN** it serves for longer than the configured shutdown timeout without a termination signal
- **THEN** `GET /healthz` continues returning success

#### Scenario: Termination signal starts bounded graceful drain

- **GIVEN** the server is serving
- **WHEN** the process receives `SIGINT` or `SIGTERM`
- **THEN** the server stops accepting new connections and exits successfully after in-flight requests drain within the configured limit
- **AND** if draining exceeds the limit, it exits non-successfully without secret-bearing output

### Requirement: Release server identity is inspectable

The server process SHALL expose its release version through its supported command-line metadata (including `--version`), and the reported version SHALL match the packaged release manifest. Version output SHALL be deterministic, short, and free of secrets or environment values.

#### Scenario: Packaged server reports matching version

- **WHEN** an operator runs the packaged server's version command
- **THEN** it reports the release version and build identity recorded in the package manifest, and the release smoke check can compare it without starting a database
