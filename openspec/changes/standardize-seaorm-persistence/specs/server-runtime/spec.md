# Spec Delta

## MODIFIED Requirements

### Requirement: Server starts only after mandatory configuration and migrations succeed

The North server process SHALL require `DATABASE_URL` and a valid 64-character hexadecimal `NORTH_OTP_HMAC_KEY`. It MAY accept `NORTH_BIND_ADDR`; when absent it SHALL bind its documented loopback default. Before accepting HTTP traffic, it SHALL validate configuration, connect to PostgreSQL, and verify that all checked-in SeaORM migrations are applied. Normal server startup SHALL NOT apply migrations, synchronize entities, or otherwise mutate the schema. It SHALL use the same server router and domain ownership rules as library/integration execution; it SHALL NOT expose an unkeyed OTP path or a second business-state implementation.

#### Scenario: Valid production-shaped startup

- **WHEN** the process receives a reachable PostgreSQL URL at the current migration head, a valid OTP HMAC key, and an optional bind address
- **THEN** it validates configuration, verifies schema readiness without applying DDL, binds the requested address, and serves the existing authenticated HTTP routes

#### Scenario: Missing or malformed OTP key fails closed

- **WHEN** `NORTH_OTP_HMAC_KEY` is missing, empty, non-hexadecimal, or not exactly 64 hexadecimal characters
- **THEN** the process exits non-successfully before accepting HTTP traffic, identifies invalid configuration without printing the key, and never issues or verifies one-time codes through an unkeyed path

#### Scenario: Database or migration failure fails before listen

- **WHEN** `DATABASE_URL` is missing, PostgreSQL is unavailable, the schema is absent or behind, or database schema state is incompatible
- **THEN** the process exits non-successfully before serving authenticated routes, reports the failure class without database credentials, directs operators to the explicit migration command when appropriate, and leaves no partially started listener

### Requirement: Server exposes safe operational readiness

After mandatory startup completes, the server SHALL expose an unauthenticated `GET /healthz` endpoint for process/liveness checks. A successful response SHALL be non-secret and SHALL mean that configuration, database connection, and current migration state were verified for that process; it SHALL NOT mean that the server applied migrations or promise continuous PostgreSQL availability. The endpoint SHALL NOT expose OTP keys, database credentials, session cookies, daemon credentials, verification codes, or arbitrary database contents.

#### Scenario: Health is unavailable before startup completes

- **WHEN** configuration, database connection, or schema-state validation is still pending or has failed
- **THEN** `GET /healthz` is not served as successful and the process does not accept normal authenticated traffic

#### Scenario: Database loss after startup does not change health semantics

- **WHEN** PostgreSQL becomes unavailable after a successful startup
- **THEN** `/healthz` remains a process/liveness signal rather than claiming continuous database readiness, authenticated requests use existing database-error behavior, and an unrecoverable process exits non-successfully

#### Scenario: Healthy process returns safe response

- **WHEN** startup completes successfully against the current migration head
- **THEN** `GET /healthz` returns a successful, non-secret response suitable for a reverse-proxy probe and authenticated routes remain governed by existing session and role checks

## ADDED Requirements

### Requirement: Server exposes an explicit migration command

The packaged server SHALL expose `north-server migrate` as an explicit deployment operation. It SHALL apply only pending versioned SeaORM migrations, SHALL NOT expose an automatic reset or schema-sync operation, and SHALL exit non-successfully without changing application data when migration preflight or migration execution fails.

#### Scenario: Operator applies pending migrations explicitly

- **WHEN** an operator runs `north-server migrate` with a reachable `DATABASE_URL`
- **THEN** the command applies pending SeaORM migrations in order, reports success, and exits without starting the HTTP listener

#### Scenario: Up-to-date schema remains unchanged

- **WHEN** an operator runs `north-server migrate` against a database already at the current migration head
- **THEN** the command succeeds without resetting or changing application tables or data
