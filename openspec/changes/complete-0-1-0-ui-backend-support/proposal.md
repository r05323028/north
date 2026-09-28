# Proposal

## Why

North already has OTP authentication, daemon setup and management, canonical Requirement transitions, and browser SSE. The 0.1.0 frontend still lacks a persistent, concurrency-safe card order, while `/auth/me` omits the already-persisted user creation time and authentication lifecycle lacks endpoint-level integration coverage.

## What Changes

- Extend the read-only current-user response with persisted `created_at`; keep role changes under existing administrator APIs and expose no session secret.
- Add focused HTTP integration coverage for OTP issuance/verification, current identity, logout invalidation, and protected-route denial; strengthen only missing daemon contract edges while retaining CLI behavior and the existing approval response contract.
- Add presentation-only per-column Requirement ordering, a board-sorted collection view, and a narrow reorder operation. Cross-column drops continue through existing explicit lifecycle operations; readiness alone promotes Discussing to Ready.
- Append a SeaORM migration that deterministically backfills existing Requirements without resetting the current development schema. Ordering never mutates Requirement content, lifecycle revision/state version, or readiness evidence; committed changes use existing `requirement.changed` hints.

Out of scope: SMTP, user settings or avatars, generic lifecycle mutation, frontend work, and unrelated refactors.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `email-auth`: define the stable, secret-free `/auth/me` profile response, including persisted creation time.
- `requirements`: define deterministic board ordering as presentation state, separate from lifecycle/content versions.
- `requirement-concurrency`: distinguish board ordering from Requirement mutations while retaining stale-state protection.
- `requirement-board-ui`: map drag-and-drop only to existing legal transition APIs and the new same-column reorder API; preserve HTTP/SSE authority.

`daemon-runtime` and `roles` behavior are already specified and implemented; this change adds regression coverage, not competing contracts.

## Impact

- Rust handlers and persistence in `crates/north-server` and `crates/north-persistence`; one append-only SeaORM migration and board-position table.
- Existing APIs remain compatible; new collection sort and reorder request are additive. Daemon approval keeps JSON preview plus authenticated same-origin POST and existing non-HTML 204 success.
- Focused PostgreSQL integration coverage, strict OpenSpec validation, and backend validation.
- Canonical documentation: `docs/product/requirement-lifecycle.md` and `docs/architecture/persistence.md`.
