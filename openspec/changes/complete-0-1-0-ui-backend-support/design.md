# Design

## Context

See proposal.md for motivation and the change specs for externally visible contracts. `requirements` persistence already centralizes aggregate writes through `lock_requirement` / `update_requirement`; readiness and clarification persistence reuse those helpers. The current collection query sorts by update time, and `/auth/me` serializes the existing `UserResponse`. The SeaORM migrator currently registers m0001 as the current baseline.

## Goals / Non-Goals

**Goals:**
- Keep profile data on the existing user record and expose only the minimal profile fields.
- Persist board rank independently from the Requirement aggregate while keeping lifecycle writes atomic with destination placement.
- Reject stale or inconsistent reorder requests and reuse the existing HTTP/SSE path.
- Upgrade current m0001 development databases without dropping or recreating them.

**Non-Goals:**
- No authentication, daemon-protocol, frontend, lifecycle-model, or email-delivery redesign.
- No generic status mutation, browser WebSocket, per-user board, pagination, or configurable ranking policy.

## Decisions

1. **Extend existing user identity.** Add persisted `created_at` to `UserRecord` and its existing response conversion; `/auth/me` remains canonical and read-only. Do not create a parallel profile model. Existing role APIs and session middleware remain unchanged. The login tests use an injected `CodeDelivery` test double; production continues using `LogCodeDelivery`.

2. **Separate presentation position from Requirement state.** Add `requirement_board_positions(requirement_id, rank)` with one row per Requirement and a foreign key to `requirements`. Lifecycle status is read only from `requirements`; the position table does not duplicate it. Rank is interpreted within the Requirement's current status column. The collection's additive `sort=board` orders by canonical status order, rank, then stable fallback keys. Existing update-time sort stays the default.

3. **Append an upgrade migration.** Keep m0001 immutable and add m0002. Backfill each existing status column with `row_number()` ordered by `(created_at, id)` and spaced by 1024. Add the table to migration verification. This supports current m0001 development databases; existing unsupported SQLx/partial pre-baseline databases retain their current manual-recreation policy. No migration path drops Requirement data.

4. **Use sparse integer ranks.** Initial/backfilled and append ranks use 1024 spacing. A reorder uses an integer midpoint where possible, or extends either boundary by 1024. If adjacent ranks leave no integer gap or arithmetic would overflow, compact only the affected status column to 1024-spaced ranks and assign the requested final order. Sort ties by `(rank, created_at, id)` and missing positions after ranked rows by `(created_at, id)`; reorder lazily materializes any missing positions before validating anchors. This avoids rewriting every row for ordinary moves while keeping fallback deterministic.

5. **Serialize board-changing writes consistently.** Acquire one PostgreSQL transaction-scoped advisory lock before locking any Requirement row in create, reorder, or existing write paths that use `lock_requirement`. The locked `update_requirement` helper compares old/new status and appends a newly transitioned card to its destination column in the same transaction. This covers explicit transitions, Ready promotion, Ready demotion, and clarification-owned updates without adding status logic to the board API. Reorder then validates `expected_state_version`, same-column references, and immediate neighbor adjacency after removing the moved card. Repeating the same placement is a no-op. `requirement.changed` is published by the handler only after commit.

6. **Keep cross-column drag on explicit operations.** The reorder route never accepts a status. Existing begin-discussion, accept, reject, request-changes, and reopen APIs remain the only user-facing transitions; readiness assessment remains the only Discussing-to-Ready path. A lifecycle operation appends the card to its new column; the frontend may then call reorder for the requested insertion point. If that second call conflicts, the committed transition remains valid and the card retains its appended destination position.

7. **Preserve daemon wire behavior.** GET approval remains a read-only JSON preview when JSON is requested. Authenticated same-origin POST remains the mutation and non-HTML success remains 204 as already specified. Tests strengthen this contract and existing ownership/revocation guarantees only where current coverage is missing.

## Risks / Trade-offs

- **One global board lock limits concurrent Requirement writes** → adequate for single-instance 0.1.0; use per-column lock keys only if throughput measurements justify the added ordering complexity. Mark the deliberate ceiling in code.
- **Sparse rank growth/collisions** → integer-gap allocation, deterministic ties, and affected-column compaction preserve correctness.
- **Old or manually inserted Requirement lacks a position row** → reads sort it deterministically after ranked rows; a serialized reorder materializes a rank before moving it.
- **m0002 applies only after current m0001** → preserve the documented rejection of unsupported SQLx/partial legacy histories and test the supported m0001-to-m0002 upgrade explicitly.
- **Shared response conversion adds `created_at` to admin user-list JSON too** → additive field sourced from the same existing user record; no authentication secret is added.

## Migration Plan

1. Add and register m0002 without changing m0001.
2. Create the presentation table and backfill existing rows deterministically in the migration transaction.
3. Run `north-server migrate` against an existing m0001 database; verify existing Requirements and deterministic positions remain present.
4. Rollback of m0002 drops only presentation ordering rows/table; it does not alter Requirement, readiness, or review records. Normal application rollback is not automatic.
