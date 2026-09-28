# Proposal

## Why

The unreleased schema evolved through 18 numbered SQL files while features were built; those files do not correspond to 18 product releases. North is preparing its first public release, so establish one current-schema baseline before any released database depends on the pre-release sequence.

## What Changes

- **BREAKING, pre-0.1.0 only:** replace migrations `0001`–`0019` with one initial-schema migration representing current schema on an empty database.
- State explicitly that databases stamped by the pre-release migration history are unsupported by the baseline and must be backed up and manually recreated if disposable. Never reset a database automatically.
- Keep future shipped migrations append-only after the baseline.
- Replace historical-upgrade coverage, which assumes old schemas must remain upgradeable, with clean-install coverage of the consolidated baseline. Keep migration/startup failure checks.
- Update migration policy, persistence/deployment/testing docs, release-qualification contract, and in-progress 0.1.0 release artifacts to make the pre-release exception explicit.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `otp-at-rest-hardening`: remove unsupported pre-0.1.0 legacy-digest migration guarantees while preserving fail-closed verification and key rotation behavior.
- `runtime-retention`: qualify expiry in the fresh baseline instead of promising a backfill from unsupported pre-release databases.

## Impact

Affected files: `migrations/`, `crates/north-server/tests/migration_upgrade.rs`, `crates/north-server/tests/fresh_install.rs`, `scripts/validate.sh`, migration and release documentation, and OpenSpec release-qualification artifacts. No runtime API or wire protocol changes. No database is reset by this change.
