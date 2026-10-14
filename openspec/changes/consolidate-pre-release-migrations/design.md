# Design

## Context

See `proposal.md` and the OTP/retention deltas; the active 0.1.0 release-qualification delta is revised in `prepare-0-1-0-release`. Before consolidation, `north-persistence` registered one compiled initial-schema baseline and two unpublished follow-on migrations for board positions and per-user preferences. Startup applies pending migrations before the server binds. `fresh_install` verified the current head and schema, while an obsolete test proved an upgrade from an unpublished intermediate schema. Release task 2.5 requires a sole baseline; this change folds both tables into the baseline and removes the follow-on migration source files and upgrade test.

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

- **One initial baseline, not one file per feature.** Fold the unpublished board-position and preference table DDL into the compiled SeaORM baseline source `initial_schema.rs`, while retaining recorded migration ID `m0001_initial_schema`. Remove follow-on pre-release migration sources and registrations. Stamp `seaql_migrations` with `north-schema:0.1.0-baseline-v1` inside the migration transaction. Before migration DDL, accept only registered applied versions forming a current migration prefix; require this marker for non-empty history, and allow an empty ledger only when no other schema relations exist. The baseline only supports empty databases, so do not backfill board ranks for unsupported intermediate schemas. Keep the final-schema catalog reference and compare every current table. Use descriptive, unnumbered Rust source filenames; migration ledger names remain explicit metadata, not source filenames.
- **Manual database disposition.** Never drop/reset a database from server startup or validation. Existing databases stamped with old migration checksums, unknown versions, or an empty ledger beside schema objects must be backed up and manually recreated only when disposable. Keep startup fail-closed. The marker identifies the consolidated migration provenance; it does not fingerprint arbitrary out-of-band schema edits.
- **Retain clean-install proof; remove historical-upgrade proof.** Assert the sole baseline head and complete current schema through `fresh_install`, including full catalog parity. Delete the obsolete test that starts from a pre-0.1.0 intermediate schema; existing domain and persistence tests continue to cover product invariants.
- **Revise active release plan, not unrelated work.** Remove only claims that old migrations are immutable or that historical upgrade coverage remains required from `prepare-0-1-0-release`. Keep its other user-authored work intact.
- **Freeze after publication.** Once the 0.1.0 baseline is released, do not edit it; append later migrations after version 1.

## Risks / Trade-offs

- **Existing pre-release database no longer starts cleanly →** reject reused/unknown history before migration DDL, document backup and manual recreation, and never auto-drop data. A fixed marker does not attest arbitrary schema changes; those remain unsupported.
- **Legacy backfill behavior loses regression coverage →** accept because no published database depends on those schemas; fresh-install qualification still proves current schema creation.
- **Single baseline transaction may expose SQL incompatibility →** run the PostgreSQL fresh-install/integration profile when an isolated `NORTH_TEST_DATABASE_URL` is available; do not claim DB validation if unavailable.
- **Release plan contradicts consolidation →** update its migration-specific design, task, and release-qualification delta in the same change.

## Migration Plan

1. Rename the baseline source to `initial_schema.rs` while retaining recorded migration ID `m0001_initial_schema`; fold unreleased board-position and preference DDL into it without historical backfill.
2. Remove follow-on migration registrations and source files; preserve preflight rejection of unsupported database histories and never reset supplied databases.
3. Keep fresh-install proof for the sole migration head and complete schema; remove the historical-upgrade test and update release artifacts and canonical persistence guidance.
4. Run strict OpenSpec validation, focused Rust/architecture checks, fast validation, and isolated PostgreSQL fresh-install/integration checks when configured. Never claim database validation if unavailable.

Rollback before publication: restore the prior migration sources and tests from version control and recreate any baseline-stamped disposable test database. No automatic database rollback is supported.
