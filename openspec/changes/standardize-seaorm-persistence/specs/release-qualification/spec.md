# Spec Delta

## MODIFIED Requirements

### Requirement: Fresh installation and startup are qualified

A release qualification run SHALL create an empty isolated PostgreSQL database and invoke the packaged `north-server migrate` command before normal server startup. It SHALL verify that the SeaORM migration head and required schema match the repository; for 0.1.0, the checked-in single initial-schema baseline SHALL reach the sole head. Normal server startup SHALL verify current migration state without applying migrations or synchronizing entities. Qualification SHALL exercise a production-shaped `DATABASE_URL` and valid `NORTH_OTP_HMAC_KEY`, and SHALL assert safe failure before routes are served when PostgreSQL is unavailable, configuration is invalid, migration execution fails, or the schema is behind. A database stamped by the unsupported pre-release SQLx history or a partial North schema SHALL be rejected without reset or data changes and documented for manual recreation. After publication, applied migrations SHALL remain immutable. Startup and failure output SHALL not disclose credentials.

#### Scenario: Empty database reaches current migration head

- **WHEN** a fresh PostgreSQL database is given to the built release
- **THEN** the packaged `north-server migrate` command applies all checked-in SeaORM migrations in order (the single initial-schema baseline for 0.1.0), records the repository migration head, and normal server startup verifies that head without applying DDL before a basic authenticated HTTP workflow can start

#### Scenario: Pre-release database is not reset automatically

- **WHEN** migration or startup targets a database stamped by unreleased SQLx history or a partial North schema
- **THEN** it exits with manual-recreation guidance before changing schema or data and does not reset the database; the operator backs up and manually recreates the database before first-release use

#### Scenario: Required configuration is exercised

- **WHEN** the built server starts with a reachable PostgreSQL database at the current SeaORM migration head and a valid 64-character hexadecimal `NORTH_OTP_HMAC_KEY`
- **THEN** it validates configuration and schema state before accepting traffic and can serve authenticated requests

#### Scenario: Mandatory startup failures are safe

- **WHEN** PostgreSQL is unavailable, explicit migration execution fails, the schema is behind, or `NORTH_OTP_HMAC_KEY` is missing or invalid
- **THEN** the migration command or server exits non-successfully before serving authenticated routes, identifies the failure class without secret material, and does not enable an unkeyed OTP path

### Requirement: Built release artifacts are reproducible and smoke-qualified

The release process SHALL build all North artifacts required by the supported 0.1.0 self-hosted topology from the exact clean commit pushed to `main` after a PR merge. It SHALL verify `HEAD` equals the full `GITHUB_SHA`, derive one matching semver from the `north-server` Cargo package and web package, publish checksums, and make source SHA and artifact versions inspectable; a Git version tag SHALL NOT be required to build. Full-history checkout SHALL make the first parent available for version-bump comparison. The build job SHALL upload package and OCI inputs under one workflow artifact ID with a SHA-256 digest and 14-day retention; secret-free preflight and qualification SHALL download that same ID and verify its contents. A built release smoke run SHALL invoke packaged `north-server migrate` against isolated PostgreSQL before starting the normal server, verify migration/startup/configuration behavior and a basic authenticated workflow, and exercise the packaged daemon/runtime boundary with the deterministic local agent fixture. Normal server startup SHALL verify the current migration head without applying DDL. The publisher SHALL consume only the already-qualified workflow artifact, SHALL NOT check out source or rebuild, SHALL NOT reference TLS secrets, and SHALL receive `packages: write` as its only write permission; no GitHub Release or tarball SHALL be created.

#### Scenario: Tag input remains immutable

- **WHEN** the workflow runs for a push to protected `main` after PR merge
- **THEN** qualification, artifact building, checksum generation, and publication all identify the same full merge commit SHA and fail if the checked-out revision or embedded versions disagree

#### Scenario: Manual dispatch uses immutable source

- **WHEN** maintainers manually dispatch qualification with a full `source_sha` for a commit already on protected `main`
- **THEN** every job checks out that exact SHA, verifies `HEAD` and package manifest match it, and GHCR publication remains unavailable because only a `main` push can publish

#### Scenario: Packaged stack passes smoke

- **WHEN** maintainers run the named 0.1.0 smoke command on the built artifacts with PostgreSQL and required configuration
- **THEN** the packaged migration command applies the checked-in schema before server startup; the server verifies the current migration head without applying DDL; authenticated HTTP works; the web artifact reaches canonical state through the supported same-origin deployment path; SSE streams without buffering; the daemon/runtime boundary connects through authenticated WSS; and the smoke run records a deterministic qualification result

#### Scenario: Same-origin proxy preserves both live transports

- **WHEN** the extracted release is served through the documented TLS/reverse-proxy route table
- **THEN** browser document and Next RSC/prefetch requests for `/requirements/{id}` reach Next.js, API JSON requests, mutations, and API subpaths reach `north-server`, cookies and SSE streaming remain intact, the daemon WSS handshake reaches the server with certificate verification enabled, and no browser WebSocket is opened

#### Scenario: Failed qualification blocks publication

- **WHEN** any explicit migration, startup, schema, authenticated workflow, browser transport, daemon boundary, artifact checksum, or version check fails
- **THEN** no release is published and the failing evidence identifies its command and artifact revision
- **THEN** no release is published and the failing evidence identifies its command and artifact revision
