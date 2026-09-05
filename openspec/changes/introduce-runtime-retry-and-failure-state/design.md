# Design: server-owned execution retry and failure projection

## 1. Terms and authority

A **logical clarification run** is the existing run/session identity and owns
the Requirement's sequential clarification slot. An **execution attempt** is
one server-created runtime command for that run: the initial `session.start`
or a later `session.resume`.

The server owns logical execution state, attempt accounting, retry policy,
command identity, and terminal failure. The daemon reports facts and performs
only transport/local recovery. The existing `daemon-protocol` delivery boundary
is retained, but this change adds its explicit handoff from generic event
receipt to the owning clarification/runtime projection; wire schemas and
transport sequencing do not change. `session.failed.recoverable` is not a retry
instruction and cannot make a run permanently failed.

Keep the existing persisted successful terminal value `Completed`. This change
specifies the retry decision states `Idle`, `Running`, `Retrying`, and `Failed`
without creating another enum/capability: `Idle` is pre-attempt/unassigned,
`Running` has a current attempt command, `Retrying` has a known failed attempt
and a pending policy decision, `Failed` is terminal logical execution, and
`Completed` remains normal successful termination.

## 2. Policy and state transitions

`max_attempts` means total durable execution-attempt commands, including the
initial `session.start`; default is 3 and is snapshotted on run creation. A
retry is eligible only while `attempt_count < max_attempts`, the run is not
cancelled, the failed outcome is known, and the pinned daemon remains an
eligible owner. Backoff uses server configuration (default base 5 seconds,
maximum 5 minutes, bounded 0–25% jitter); the calculated due timestamp is
persisted, so restart never recomputes an already selected delay.

| Event/condition | Durable server result | Logical run/slot |
| --- | --- | --- |
| initial start command committed | attempt 1, state `Running` | active, slot occupied |
| `session.started` accepted | current attempt running | active, slot occupied |
| known `session.failed`, budget remains | state `Retrying`, safe reason, `next_retry_at` | non-terminal, slot occupied |
| due retry command committed | next attempt, state `Running`, clear due time | active, slot occupied |
| known failure with no budget | state `Failed`, safe terminal reason | terminal, slot released |
| `execution_outcome_unknown` | state `Failed`, safe unknown reason; no resume | terminal, slot released |
| successful `session.completed` | existing `Completed` | terminal, slot released |
| cancellation while no current attempt | terminal safe `cancelled` result | terminal, slot released |

A failure never changes Requirement content, lifecycle, revision, readiness, or
state version. Assessment events remain separately revision-bound.

The server may use failure classification and daemon recoverability as policy
inputs, but neither is authoritative alone. Known transient/permitted failures
follow the bounded policy; unknown outcome always takes the no-resubmit path.
Raw daemon/provider/runtime error text is not part of the public projection.

## 3. Durable storage and transaction boundaries

Extend the existing execution-session row with at least:

- `attempt_count` (0 before assignment; incremented at command creation),
- snapshotted `max_attempts`,
- `next_retry_at` nullable,
- safe last-failure class/reason, and
- the current attempt/command identity as needed by existing command binding.

Add a narrow `execution_attempts` table, not a generic workflow engine:

- `session_id`, `attempt_number`, `command_id`, `command_kind`, and `created_at`;
- outcome/failure linkage and safe failure class; and
- nullable `failure_event_id` with a uniqueness constraint.

Enforce unique `(session_id, attempt_number)`, unique `command_id`, and unique
failure-event identity. The attempt row and command-outbox row commit in the
same transaction. Initial `session.start` creation and each `session.resume`
creation use the same transaction-aware persistence path. A helper that creates
a command in one transaction and increments the counter later is forbidden.

Existing event identity/sequence dedupe remains the outer boundary. Accepted,
rejected, and duplicate failure facts record/return their known ACK only after
this state transaction commits.

## 4. Due retry scheduling

Persisted `next_retry_at` is the schedule. A small server worker scans an index
on `(state, next_retry_at)` at startup and on a bounded interval, and is woken
when failure handling creates a due row. No in-memory timer is authoritative.
Startup therefore recovers future and already-due retries from PostgreSQL.

A worker claims work with a session row lock (or equivalent conditional update)
and `SKIP LOCKED` for batch discovery. Inside one transaction it verifies:
state is `Retrying`, due time has arrived, cancellation is false, no current
attempt exists, and budget remains; then it creates exactly one durable
`session.resume`, attempt row, and counter update. The unique constraints and
row lock make two workers/request paths harmless. A stale timer sees the new
state and does nothing.

A disconnected but still registered pinned daemon does not block durable command
creation: the resume is addressed to its immutable owner and remains in the
outbox for reconnect delivery. Reconnect only replays that command; it never
creates an attempt. If the owner is revoked/no longer eligible, the server does
not migrate the run and applies the terminal `owner_unavailable` policy result.
A reconnect and due worker race is serialized by the session row lock.

The scheduler is a single bounded worker loop plus existing outbox machinery,
not a general job/scheduler abstraction. Multiple server workers/processes use
the database lock/constraints as the authority.

## 5. Attempt identity and failure idempotency

Attempt count increments exactly once when a new durable `session.start` or
`session.resume` command identity is committed. It does not increment for
WebSocket delivery, command ACK retry, reconnect, frame replay, heartbeat,
reconciliation, daemon journal replay, or event-journal replay. Cancel and
message commands are not execution attempts.

A `session.failed` event is bound to the current attempt/run and processed in
the same transaction as its policy effect. A duplicate event identity returns
the original ACK/outcome and cannot decrement budget, schedule another retry,
create another resume, or repeat terminalization. Same sequence with a different
identity remains a protocol conflict. A failure that loses a database race is
re-read and treated as already handled, not applied twice.

The server maps the fact to bounded safe classes such as `runtime_failure`,
`execution_outcome_unknown`, `retry_exhausted`, `owner_unavailable`, or
`cancelled`; browser/API reads never expose raw provider errors, command
payloads, daemon IDs, or credentials.

## 6. Cancellation and unknown outcomes

Cancellation is run-scoped and locks the explicit `run_id`:

- `Running`: set `cancel_requested`, create/reuse one durable pinned
  `session.cancel`, and wait for existing terminal runtime fact. A later failure
  cannot schedule a retry.
- `Retrying` before a resume command: atomically mark terminal `cancelled`,
  clear `next_retry_at`, and release the slot; no cancel command is needed.
- Waiting for a due time: same terminal cancellation; stale worker/timer checks
  state and cannot resurrect it.
- Pinned daemon unavailable: durable cancel remains addressed to that owner; no
  migration and no future resume.
- Unknown outcome/terminal run: do not resubmit or resurrect. A cancel request is
  idempotent/no-op against that run and cannot affect a newer run.

If a due worker wins the lock first, the run becomes `Running`; cancellation
then follows the first case. If cancellation wins first, the worker creates
nothing. A successful cancellation still uses existing `session.completed`; a
failure/terminal cancellation uses safe `cancelled`/operational failure
projection without introducing a `session.cancelled` frame.

`execution_outcome_unknown` never gets automatic retry, even when the persisted
attempt budget remains. Any later attempt must be a new explicit server command
and must pass normal policy; this change exposes no browser action that silently
does that.

## 7. Public clarification projection

Extend the existing `/requirements/{id}/session` projection; do not add an
execution-state endpoint or a second browser state machine. Safe fields are:
`attempt_count`, nullable `next_retry_at`, and nullable bounded
`failure_reason`, alongside existing run fields. Do not expose `max_attempts`,
remaining budget, daemon/provider identity, raw reason, or runtime operation
IDs.

| Server condition | `phase` | `status` |
| --- | --- | --- |
| no assignment/attempt | `awaiting_assignment` | `unavailable` |
| current start/resume not yet acknowledged by runtime | `active` | `starting` |
| current attempt running and owner live | `active` | `running` |
| current attempt running and owner offline | `active` | `unavailable` |
| policy selected retry; due time pending or owner offline | `active` | `retrying` |
| normal completion | `terminal` | `completed` |
| exhausted, unknown, owner-unavailable, or cancelled logical run | `terminal` | `failed` |

`Retrying` is server-internal; public `retrying` is a safe projection status,
not a new source of truth. `phase=active`/`status=retrying` retains the
sequential slot. `phase=terminal`/`status=failed` releases it. The browser
renders the safe reason only from the bounded server enum and does not infer it
from transcript/activity.

## 8. Validation focus

The implementation must prove transaction order, duplicate/replay behavior,
restart recovery, worker races, pinned ownership, cancellation races, unknown
outcome no-resubmit, Requirement isolation, and safe browser mapping. The
implementation checklist is in `tasks.md`.
