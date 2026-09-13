# Introduce runtime-event retention

## Why

North 0.1 stores runtime telemetry in PostgreSQL instead of object storage,
so activity telemetry grows forever unless something boring deletes it.
Retention must erase every ephemeral byte without weakening durable product
truth or durable coordination state — including the execution-attempt,
retry-counter, due-scheduling, and failure-classification state introduced by
migration 0016.

## What Changes

- Explicit ephemeral allowlist: `clarification_activities` (coarse activity
  telemetry) gains an indexed `expires_at`; no other class is a retention
  target, and new tables are durable unless deliberately allowlisted.
- Bounded, ordered, idempotent sweeps behind named persistence operations only
  — batched, `FOR UPDATE SKIP LOCKED`, safe across concurrent server
  instances, with no generic table-name deletion API.
- Typed retention configuration (window, cadence, batch bound) with documented
  0.1.0 defaults and explicit validation.
- Durable-table firewall: a structural allowlist test over persistence
  `DELETE FROM` sites plus an amnesia integration proof that purging all
  eligible telemetry leaves canonical projections identical.
- Documentation of durable versus ephemeral classes as enforced, not
  aspirational.

## Capabilities

### New Capabilities

- `runtime-retention`: ephemeral allowlist, expiry semantics, bounded sweep
  mechanics, configuration validation, and the durability firewall.

### Modified Capabilities

- `clarification-runtime`: accepted `agent.activity` rows are ephemeral telemetry
  with an expiry, while dedupe/outcome records, ACK watermarks, execution
  attempts, retry scheduling, and failure classification remain durable; a
  replayed expired activity event stays deduplicated.
- `requirement-conversation-workspace`: activity display is best-effort and may
  expire without changing conversation, readiness, review, or session truth.

## Impact

- Affected docs: docs/architecture/persistence.md (retention mechanics and
  classification), docs/development/testing.md (integration coverage),
  docs/development/invariants.md (persistence and retention rows).
- Migration: adds `0017_runtime_event_retention` (nullable expiry column,
  deterministic backfill, NOT NULL enforcement, sweep index).
- Durable-side prerequisites already landed:
  introduce-runtime-retry-and-failure-state (migration 0016) and
  introduce-agent-requirement-clarification (activity event source).
