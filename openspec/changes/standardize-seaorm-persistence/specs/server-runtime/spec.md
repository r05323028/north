# Spec Delta

## MODIFIED Requirements

### Requirement: Server starts only after mandatory configuration and migrations succeed

The North server process SHALL require `DATABASE_URL` and a valid 64-character hexadecimal `NORTH_OTP_HMAC_KEY`. It MAY accept `NORTH_BIND_ADDR`; when absent it SHALL bind its documented loopback default. Before accepting HTTP traffic, it SHALL validate configuration, connect to PostgreSQL, run the guarded versioned migration path, verify and commit the current SeaORM head, then build the app and bind. The migration path SHALL serialize with the explicit CLI through a PostgreSQL transaction-scoped advisory lock. It SHALL apply only checked-in migrations; normal startup SHALL NOT synchronize entities, reset, or drop schema. The server database role therefore SHALL have the DDL privileges needed by pending migrations. The process SHALL NOT expose an unkeyed OTP path or a second business-state implementation.

#### Scenario: Valid production-shaped startup

- **WHEN** the process receives a reachable PostgreSQL URL at an empty, pending, or current supported schema, a valid OTP HMAC key, and an optional bind address
- **THEN** it validates configuration, applies only pending migrations under the shared lock, verifies and commits the current head, then binds and serves the existing authenticated HTTP routes

#### Scenario: Missing or malformed OTP key fails closed

- **WHEN** `NORTH_OTP_HMAC_KEY` is missing, empty, non-hexadecimal, or not exactly 64 hexadecimal characters
- **THEN** the process exits non-successfully before accepting HTTP traffic, identifies invalid configuration without printing the key, and never issues or verifies one-time codes through an unkeyed path

#### Scenario: Database or migration failure fails before listen

- **WHEN** `DATABASE_URL` is missing, PostgreSQL is unavailable, migration preflight rejects the schema, migration execution or commit fails, or the database role lacks required DDL privileges
- **THEN** the process exits non-successfully before serving authenticated routes, reports the failure class without database credentials, and leaves no partially started listener

### Requirement: Server exposes safe operational readiness

After mandatory startup completes, the server SHALL expose an unauthenticated `GET /healthz` endpoint for process/liveness checks. A successful response SHALL be non-secret and SHALL mean configuration and database connection succeeded and the current migration head was verified and committed for that process; it SHALL NOT promise continuous PostgreSQL availability. The endpoint SHALL NOT expose OTP keys, database credentials, session cookies, daemon credentials, verification codes, or arbitrary database contents.

#### Scenario: Health is unavailable before startup completes

- **WHEN** configuration, database connection, migration, verification, or commit is still pending or has failed
- **THEN** `GET /healthz` is not served as successful and the process does not accept normal authenticated traffic

#### Scenario: Database loss after startup does not change health semantics

- **WHEN** PostgreSQL becomes unavailable after a successful startup
- **THEN** `/healthz` remains a process/liveness signal rather than claiming continuous database readiness, authenticated requests use existing database-error behavior, and an unrecoverable process exits non-successfully

#### Scenario: Healthy process returns safe response

- **WHEN** startup completes successfully against a supported schema
- **THEN** `GET /healthz` returns a successful, non-secret response suitable for a reverse-proxy probe and authenticated routes remain governed by existing session and role checks

### Requirement: Server exposes an explicit migration command

The packaged server SHALL expose `north-server migrate` as an optional operator operation using the same guarded, serialized migration path as startup. It SHALL apply only pending versioned SeaORM migrations, SHALL NOT expose an automatic reset or schema-sync operation, and SHALL exit non-successfully without changing application data when migration preflight or execution fails.

#### Scenario: Operator applies pending migrations explicitly

- **WHEN** an operator runs `north-server migrate` with a reachable `DATABASE_URL`
- **THEN** the command applies pending SeaORM migrations in order, reports success, and exits without starting the HTTP listener

#### Scenario: Up-to-date schema remains unchanged

- **WHEN** an operator runs `north-server migrate` against a database already at the current migration head
- **THEN** the command succeeds without resetting or changing application tables or data
