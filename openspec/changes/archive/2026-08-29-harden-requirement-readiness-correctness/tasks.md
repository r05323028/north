## 1. Domain and schema contract

- [x] 1.1 Add a new versioned migration that adds positive `requirements.state_version` with safe backfill to 1; do not rewrite prior migrations.
- [x] 1.2 Extend `north-domain::Requirement` restoration, creation, accessors, edit outcomes, and lifecycle operations so `revision` remains content-only and `state_version` increments exactly once for every real mutation, including Ready demotion, with no-op and rejected paths inert.
- [x] 1.3 Add domain tests for initial state version, lifecycle-only transitions, real edits, Ready demotion, no-op edits, terminal refusal, and positive restoration validation.
- [x] 1.4 Make revision and state-version increments checked so exhausted tokens fail without partial mutation.
- [x] 1.5 Run `cargo fmt --all --check`, domain/persistence unit tests, and SQL lint for the new migration.

## 2. Transactional persistence

- [x] 2.1 Update Requirement row mapping, inserts, selects, list queries, and atomic updates to persist and compare `state_version` while continuing to match readiness evidence on `requirement_revision`.
- [x] 2.2 Centralize mutation persistence so direct edits, Begin Discussion, review transitions, Reopen, and readiness promotion each compare expected state version and bump it once without handler-side duplicate increments.
- [x] 2.3 Make review transition persistence lock and verify current revision, Ready state generation, expected state version, and supplied assessment identity before writing one audit row.
- [x] 2.4 Preserve assessment transaction ordering, immutable evidence, event identity/deduplication/sequence behavior, session binding, and ACK-after-commit while incrementing state version only for successful promotion.
- [x] 2.5 Persist accepted assessment `state_version` and use exact-generation queries for packets and review transitions; make evidence Requirement deletion restrictive.
- [x] 2.6 Run `cargo clippy --workspace --all-targets -- -D warnings` and persistence unit tests.

## 3. Server API and authorization

- [x] 3.1 Replace existing-Requirement mutation DTO concurrency fields with `expected_state_version`; expose `revision` and `state_version` consistently in Requirement responses and map conflicts to HTTP 409.
- [x] 3.2 Add `assessment_id`, `requirement_revision`, and `requirement_state_version` to review packets; require assessment identity plus expected state version for Accept, Reject, and Request Changes, and expected state version for Reopen.
- [x] 3.3 Split edit normalization so title/description/list entries remain non-empty while summary and list fields accept intentional empty values; preserve bounded trimming and reject invalid entries.
- [x] 3.4 Enforce/document workspace-wide authenticated Requirement and conversation visibility/collaboration without introducing ACLs; retain reviewer-role checks before review mutation.
- [x] 3.5 Update all in-repository API callers, fixtures, and server unit tests from `expected_revision` to `expected_state_version`.
- [x] 3.6 Run server unit tests, architecture tests, and API-focused checks.

## 4. Regression and integration coverage

- [x] 4.1 Add database-backed stale-review integration coverage using real Begin Discussion, structured edit, readiness assessment A, Request Changes, assessment B at unchanged content revision, stale Accept 409, and current Accept success.
- [x] 4.2 Add database-backed coverage for state-version increments, rejected/duplicate assessments, no-op edits, Ready demotion, summary clearing, stale edit side-effect absence, and reviewer/requester authorization.
- [x] 4.3 Replace applicable manual Ready SQL setup with real lifecycle/readiness setup and assert audit/status/evidence invariants through APIs.
- [x] 4.4 Add assertions for persisted accepted generations and restrictive evidence deletion.
- [x] 4.5 Run isolated `./scripts/validate.sh integration` with `NORTH_TEST_DATABASE_URL` and inspect failures before continuing.

## 5. Specifications and documentation

- [x] 5.1 Update canonical OpenSpec requirements, readiness, and conversations specs to distinguish content `revision` from mutable `state_version`, define workspace collaboration, and bind review decisions to assessment identity.
- [x] 5.2 Update product lifecycle/readiness/conversation docs and persistence/server architecture docs with the two-token contract, migration behavior, and review packet identity.
- [x] 5.3 Run `openspec validate --all --strict`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, web lint/typecheck/build if DTO consumers changed, `git diff --check`, and SQL lint.
