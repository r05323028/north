# Tasks

## 1. Auth and Profile

- [x] 1.1 Extend the existing user record and current-user response with persisted `created_at`; verify `/auth/me` returns canonical identity without session or credential secrets.
- [x] 1.2 Add HTTP integration coverage for request-code delivery, successful verification, invalid and expired codes, first-user Owner identity, and `/auth/me` serialization; verify the existing `CodeDelivery` abstraction and log delivery remain unchanged.
- [x] 1.3 Verify logout clears the cookie and invalidates a copied session; assert `/auth/me` and a protected API return 401 after logout, and profile requests cannot change role.

## 2. Daemon Contract Regression

- [x] 2.1 Audit the existing setup integration test and add only missing assertions for JSON preview, authenticated same-origin approval's existing 204 response, setup authorization, and credential-free daemon list responses.
- [x] 2.2 Verify request, preview, approval, one-time poll/claim, WebSocket connect, ownership-scoped list, revoke, active-connection closure, and reconnect refusal remain covered without changing CLI contracts.

## 3. Persistent Board Ordering

- [x] 3.1 Add and register m0002 with a separate board-position table and deterministic `(status, created_at, id)` backfill; update migration verification and prove upgrade from existing m0001 preserves Requirement rows.
- [x] 3.2 Allocate destination positions atomically on creation and every existing status-changing persistence path, including readiness promotion and Ready demotion; prove ordering leaves revision, state_version, and review evidence unchanged.
- [x] 3.3 Add `sort=board` to the collection API and deterministic missing/colliding-rank fallback; verify default updated-time sorting stays backward compatible.

## 4. Reorder API and Concurrency

- [x] 4.1 Implement same-column `POST /requirements/{id}/reorder` with required expected state version, adjacent before/after anchors, idempotent no-op, sparse ranks, and affected-column rebalance; reject invalid, cross-column, and stale targets.
- [x] 4.2 Serialize reorder with create and Requirement state writes using one consistent transaction lock; publish the existing `requirement.changed` hint only after a persisted reorder commits.
- [x] 4.3 Add PostgreSQL tests for reorder/reload, concurrent stale reorder and reorder-versus-transition, invalid anchors, deterministic fallback, and order changes on a Ready Requirement preserving its assessment/review packet.

## 5. Lifecycle, Documentation, and Validation

- [x] 5.1 Verify board drops use only existing explicit transitions; cover Draft to Discussing, no manual Discussing to Ready, reviewer/assessment-gated Ready decisions, feedback-required Request Changes, reviewer Reopen, and Accepted terminality.
- [x] 5.2 Update `docs/product/requirement-lifecycle.md` and `docs/architecture/persistence.md` for the domain/presentation boundary and m0002 upgrade policy.
- [x] 5.3 Run formatting, lint, architecture and strict OpenSpec checks; run migration upgrade tests and the full backend/PostgreSQL integration suite; inspect the final diff and changed-file set.
