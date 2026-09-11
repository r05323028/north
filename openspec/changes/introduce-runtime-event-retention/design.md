# Design

## Data classification (authoritative)

Ephemeral allowlist — the only expiry-driven retention targets:

| Table | Semantics |
| --- | --- |
| clarification_activities | Coarse agent/tool activity for observability; never canonical execution or product truth |

Durable by default — every class not on the allowlist, including:

- Business: users, roles, requirements, transition_audit, conversations,
  messages, readiness_assessments, repositories, instance_settings.
- Coordination: daemon_registrations, daemon_setup_requests (own bounded 24h
  cleanup), execution_sessions, execution_attempts, server_command_outbox,
  server_command_tombstones, server_message_command_map, server_event_dedupe,
  and all sequence watermarks/sparse ACK state.
- Auth lifecycle: verification_codes and sessions expire through their own
  auth semantics and are not runtime telemetry.

Migration 0016 state (`attempt_count`, `max_attempts`, `next_retry_at`,
`failure_class`, `current_attempt_id`, `execution_attempts`) is durable
coordination and is explicitly NOT a retention target.

## Decisions

- Allowlist by construction: persistence exposes only named ephemeral
  maintenance operations with fixed SQL. No host-facing API can name a table
  or run arbitrary deletion, so a new table is safe by default.
- Expiry: `clarification_activities.expires_at TIMESTAMPTZ NOT NULL`, written
  at insert from the configured retention window; eligibility is
  `expires_at <= CURRENT_TIMESTAMP`.
- Migration 0017 backfills deterministically:
  `expires_at = created_at + INTERVAL '7 days'` (the 0.1.0 default window),
  then enforces NOT NULL so no later insert can bypass expiry treatment.
  Legacy activity older than the default window becomes immediately eligible.
- Sweep statement:
  `WITH expired AS (SELECT id FROM clarification_activities
   WHERE expires_at <= CURRENT_TIMESTAMP ORDER BY expires_at ASC, id ASC
   LIMIT $1 FOR UPDATE SKIP LOCKED) DELETE ... USING expired ...`,
  backed by an `(expires_at ASC, id ASC)` index. This statement is the single
  deletion primitive and is always batch-bounded.
- Bounded drain: one scheduler cycle runs sweep passes while each pass deletes
  a full batch, stops when a pass deletes fewer rows than the batch bound, and
  stops at the configured maximum passes per cycle (default 20 → 10,000 rows
  per cycle at default batch size). Every pass commits its own short
  transaction, so a cycle never becomes one large transaction and cannot loop
  unbounded. The post-pass backlog check is a bounded `LIMIT 1` existence
  probe, never an exact `COUNT(*)`, so maintenance work per cycle is strictly
  bounded even with a very large backlog. Cycle throughput therefore recovers
  from backlogs larger than a single batch while remaining bounded per tick.
- Concurrency and transactions: `FOR UPDATE SKIP LOCKED` mirrors the retry
  worker claim pattern; each pass takes only row locks on candidate rows and
  never runs table-wide locks or unindexed scans. Concurrent server instances
  delete disjoint rows; repeats are idempotent and cannot touch non-expired
  rows.
- Failure behavior: the database is the sole expiry authority; a failed cycle
  is logged and retried on the next cadence tick. A pass bound reached with
  expired rows still eligible reports that rows remain and the next cycle
  continues, so lag is observable rather than silent. Restart loses nothing and
  requires no replay or repair.
- Scheduling: one additional Tokio interval task beside the existing retry
  worker (`start_retention_worker`). No new job framework, no second source of
  truth.
- Configuration: `RetentionConfig { retention_seconds,
  sweep_interval_seconds, batch_size, max_passes_per_cycle }` with 0.1.0
  defaults 7 days / 60 s / 500 rows / 20 passes, constructed through a
  validating constructor that rejects non-positive window or cadence and batch
  or pass bounds outside their allowed ranges. The drain result exposes only
  `deleted`, `passes`, and `drain_limit_reached`, which is true only when
  expired rows remain after the pass budget; the bounded probe
  `expired_activity_remains()` returns a boolean. Exact backlog size is left to
  a future observability concern rather than the retention progress path.
- Observability: cycle failures, drained row counts, and a drain-limit warning
  that expired rows remain are reported through the server log, matching the
  retry worker posture, without running an exact backlog count in the worker
  path.
- Configuration exposure: server startup builds routes through
  `build_app_with_retention`, and `build_app` delegates with the validated
  defaults. Retention settings apply prospectively — already-persisted activity
  rows keep the expiry computed when they were written, while the window drives
  future inserts and the cadence, batch bound, and pass bound drive future
  cycles.

## Durable-table firewall

1. Named operations only: the retention API is a single named sweep (plus its
   bounded drain and a read-only backlog count) with fixed SQL; hosts never
   hand-roll SQL (existing ownership mapping).
2. Structural test: persistence Rust syntax is parsed with `syn`. Every SQL
   argument to a known SQL-executing call (`query`, `query_as`, `query_scalar`,
   `query_unchecked`, `query_as_unchecked`, `raw_sql`) must be a direct static
   string literal, so production persistence SQL is statically inspectable and
   `format!`, `concat!`, variables, and fragmented expressions are rejected
   whether or not they look destructive. Destructive statements must
   additionally match the exact (source file, table) classification —
   `retention.rs` → `clarification_activities` (TTL allowlist), `daemon.rs` →
   `daemon_setup_requests` (setup-row retention), `delivery.rs` →
   `server_command_outbox` (acknowledged outbox compaction) — and `TRUNCATE`,
   dynamic table names, and destructive literals from the wrong file are always
   rejected. Case and ASCII whitespace are normalized so multiline or irregular
   formatting cannot hide a statement, and `cfg(test)` items are not production
   code.
3. Amnesia integration test: purging all eligible telemetry leaves
   Requirement/board projections, conversation history, readiness and review
   packet (including a real human review decision on a second Requirement),
   clarification run projection, attempt/retry/failure state, repository
   provenance, user/role rows, daemon setup rows, and outbox/dedupe/watermark
   protocol state identical. A custom-window store proves `expires_at` follows
   the configured window, and replaying a purged activity event must return the
   recorded duplicate without recreating telemetry.

## Open Questions

(none — resolved during refinement)

## Merge-order note

The two MODIFIED capability deltas are generated against the canonical text on
`main`. If the pending archive of `introduce-runtime-retry-and-failure-state`
lands first, its canonical `clarification-runtime` wording replaces the
requirement this change modifies; refresh the delta base and re-apply the
retention additions instead of overwriting the retry semantics.
