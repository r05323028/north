# Design

## Context

See `proposal.md` and the OTP/retention deltas; the active 0.1.0 release-qualification delta is revised in `prepare-0-1-0-release`. The checked-in history contains 18 migrations numbered through 0019. `north-persistence` embeds them at compile time; startup applies them before the server binds. `fresh_install` verifies the current migration head. `migration_upgrade` proves data-preserving upgrades from unpublished intermediate schemas. The in-progress 0.1.0 release change currently treats that history as immutable; its relevant plan text must be revised alongside this change.

## Goals / Non-Goals

**Goals:**
- Make clean install use one migration representing current schema.
- Explicitly reject automatic compatibility/reset for pre-0.1.0 databases.
- Keep post-release migrations append-only.
- Preserve fresh-install and startup-failure evidence.

**Non-Goals:**
- Resetting, connecting to, or modifying any developer/shared database.
- Supporting upgrades from unpublished migration histories.
- Changing runtime/domain behavior or normalizing every historical SQL statement into a hand-written schema.

## Decisions

- **One initial baseline, not one file per feature.** Concatenate existing migration SQL in original version order into `0001_initial_schema.sql`, with source-version comments. This preserves the exact clean-install DDL and constraints while discarding the unsupported intermediate migration history. Legacy backfills run against empty tables during baseline installation; do not keep an upgrade test for those nonexistent supported database states.
- **Manual database disposition.** Never drop/reset a database from server startup or validation. Existing databases stamped with old migration checksums must be backed up and manually recreated only when disposable. Keep startup fail-closed.
- **Retain clean-install proof; remove historical-upgrade proof.** Assert the single migration head and representative final schema through `fresh_install`; delete `migration_upgrade` and remove its integration invocation. Existing domain and persistence tests continue to cover product invariants.
- **Revise active release plan, not unrelated work.** Remove only claims that old migrations are immutable or that historical upgrade coverage remains required from `prepare-0-1-0-release`. Keep its other user-authored work intact.
- **Freeze after publication.** Once the 0.1.0 baseline is released, do not edit it; append later migrations after version 1.

## Risks / Trade-offs

- **Existing pre-release database no longer starts cleanly →** document backup and manual recreation; never auto-drop data.
- **Legacy backfill behavior loses regression coverage →** accept because no published database depends on those schemas; fresh-install qualification still proves current schema creation.
- **Single baseline transaction may expose SQL incompatibility →** run the PostgreSQL fresh-install/integration profile when an isolated `NORTH_TEST_DATABASE_URL` is available; do not claim DB validation if unavailable.
- **Release plan contradicts consolidation →** update its migration-specific design, task, and release-qualification delta in the same change.

## Migration Plan

1. Replace pre-release migration files with one baseline by preserving their SQL order; update migration metadata and canonical docs.
2. Keep fresh-install coverage, remove obsolete historical-upgrade test and invocation, and update the release qualification artifacts to state the pre-release reset boundary.
3. Run strict OpenSpec validation, focused Rust/architecture tests, fast validation, and isolated PostgreSQL fresh-install/integration checks when configured. Never reset the supplied database.

Rollback before publication: restore old migration files and tests from version control and recreate any baseline-stamped disposable test database. No automatic database rollback is supported.
