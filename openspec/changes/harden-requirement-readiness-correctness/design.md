## Context

See `proposal.md` for motivation. Current Requirement rows expose `revision` as content revision, while lifecycle updates and review decisions use it as the only optimistic-concurrency token. Readiness evidence already has immutable, revision-bound rows and transactional event handling; this change extends that transaction without moving business rules out of `north-domain`.

## Goals / Non-Goals

**Goals:**

- Make `state_version` the single optimistic-concurrency token for every real existing-Requirement mutation while preserving `revision` as content identity.
- Make review decisions prove both current mutable state and exact readiness evidence represented by the packet.
- Keep mutation/version calculation centralized, with one transactional compare-and-update path per operation.
- Make workspace-wide collaboration and edit normalization explicit and testable.

**Non-Goals:**

- No per-Requirement ACL, ownership model, review-packet storage, or content-revision semantic change.
- No mutation of readiness evidence rows, replay-based state reconstruction, or relaxation of session binding, deduplication, sequence, or ACK ordering.
- No backward-compatible support for ambiguous `expected_revision` mutation fields in the pre-0.1.0 HTTP API.

## Decisions

### Separate content and mutable-state tokens

Add positive `state_version: i64` to the domain and `requirements` table, initialized to 1. Content edits advance both `revision` and `state_version`; lifecycle-only mutations advance only `state_version`; no-op and rejected operations advance neither. Readiness rows continue storing and comparing `requirement_revision`; accepted evidence additionally stores the post-promotion `accepted_state_version` generation.

Incrementing `revision` for lifecycle changes was rejected because it would make assessment evidence stale for a status-only mutation and violate content identity. A timestamp was rejected because it is not deterministic enough for compare-and-swap.

### Centralize mutation preparation in the domain

Keep lifecycle legality, edit normalization results, and next-token calculation in `north-domain`. Each real operation produces the updated Requirement and prior state-version precondition. `north-persistence` performs one `UPDATE ... WHERE id = $1 AND state_version = $expected` inside the caller transaction and maps zero rows to conflict. Handlers never increment versions or issue ad-hoc state updates.

Handler-side increments and database triggers were rejected: the former duplicate invariants across direct, conversation, review, and readiness paths; the latter cannot express domain no-op and status legality clearly.

### Bind human review to current evidence

Review packets return `assessment_id`, `requirement_revision`, and `requirement_state_version`. Accept, Reject, and Request Changes carry `assessment_id` and `expected_state_version`. Persistence locks the Requirement and verifies expected state version, current revision, Ready state, exact accepted assessment generation (`accepted_state_version`), and assessment identity before applying the domain transition and audit row in one transaction. Reopen compares state version only.

The assessment identity is not copied into Requirement content. A new Ready promotion at the same content revision gets a new assessment row and state-version generation, so an old packet cannot decide the new state.

### Preserve readiness transaction order

Assessment handling remains: authenticate and bind session; validate identity, deduplicate, and check sequence; lock Requirement; compare `requirement_revision`; run domain gates; insert immutable evidence; apply valid Ready promotion and one state-version increment; insert rejection/transition records; commit; then construct and send ACK. Duplicate or rejected facts do not call the mutation update path.

### Keep authorization simple and explicit

There is no per-Requirement ACL in 0.1. Authenticated users in an instance can read Requirements and conversations. Requesters can create, edit non-terminal Requirements, begin discussion, and append context. Manager/Admin/Owner retain those capabilities and alone pass review guards. Tests assert this policy at HTTP boundaries.

### Normalize optional fields independently

Use non-empty trimming for title, description, and each list item. Use bounded trimming that permits empty summary and empty list values. The domain decides whether normalized values are a no-op; persistence and conversation append happen only for a real accepted mutation.

## Risks / Trade-offs

- [Migration on existing rows] → Add versioned migrations with `DEFAULT 1` and positive checks for Requirement state and accepted assessment generation; verify startup and restored rows before writes. Keep evidence foreign-key deletion restrictive.
- [Concurrent review/readiness race] → Lock Requirement, compare state version and assessment identity in one transaction, and add the real API race integration test.
- [API replacement] → This is pre-0.1.0; update canonical examples, tests, and all in-repo callers together instead of accepting two concurrency fields.
- [Workspace-wide collaboration] → Any authenticated instance user with a known id can edit non-terminal state; retain reviewer role checks until a future ACL capability is specified.

## Migration Plan

1. Apply migration 0010; existing Requirements receive `state_version = 1`.
2. Apply migration 0011; only a unique accepted assessment for a currently Ready Requirement is backfilled to the current state version. Other legacy accepted rows receive `generation_unknown` and remain excluded from review; Requirement deletion no longer cascades changes into evidence.
3. Deploy code that reads/writes both tokens and records accepted promotion state versions.
4. Existing readiness evidence remains revision-bound; unknown legacy generations require a fresh assessment/rebaseline before review.
5. Roll back application code before the migrations if needed; additive columns can remain unused. Do not rewrite or delete prior migrations.
