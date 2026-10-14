# Proposal

## Why

The unreleased schema evolved through 18 numbered SQL files while features were built; those files do not correspond to 18 product releases. North is preparing its first public release, so establish one current-schema baseline before any released database depends on the pre-release sequence.

## What Changes

- **BREAKING, pre-0.1.0 only:** replace unpublished follow-on migrations with one compiled SeaORM initial-schema migration representing the complete current schema on an empty database. Keep recorded head `m0001_initial_schema` in the migration ledger; use descriptive, unnumbered source `initial_schema.rs`.
- State explicitly that databases stamped by the pre-release migration history are unsupported by the baseline and must be backed up and manually recreated if disposable. Preflight and verification reject the reused `m0001_initial_schema` ID without the consolidated-baseline marker, unregistered history, and partial schemas before migration DDL. Never reset a database automatically.
- Keep future shipped migrations append-only after the baseline and use descriptive, unnumbered Rust source filenames.
- Replace historical-upgrade coverage, which assumes old schemas must remain upgradeable, with clean-install coverage of the consolidated baseline. Keep migration/startup failure checks.
- Update migration policy, persistence/deployment/testing docs, release-qualification contract, and in-progress 0.1.0 release artifacts to make the pre-release exception explicit.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `otp-at-rest-hardening`: remove unsupported pre-0.1.0 legacy-digest migration guarantees while preserving fail-closed verification and key rotation behavior.
- `runtime-retention`: qualify expiry in the fresh baseline instead of promising a backfill from unsupported pre-release databases.

## Impact

Affected source files: `crates/north-persistence/src/initial_schema.rs` and `migrations.rs`, `crates/north-server/tests/fresh_install.rs` and its current-schema reference fixture, migration and release documentation, and OpenSpec release-qualification artifacts. No runtime API or wire protocol changes. No database is reset by this change.
