## Purpose

Refines the canonical `execution-retry-authority` contract so execution failure
facts, durable retry scheduling, attempt identity, cancellation, and public
clarification projection cannot diverge into a second state machine.

## MODIFIED Requirements

### Requirement: Server owns execution state and retry policy

For every logical clarification run, the server SHALL authoritatively persist
execution state (`Idle`, `Running`, `Retrying`, or `Failed`), attempt count,
snapshotted retry limit, due retry time, and a bounded safe failure reason.
Existing successful `Completed` persistence remains the terminal success value.
The server SHALL decide whether to issue `session.resume` and when execution
becomes terminal `Failed`; the daemon SHALL never make that decision.

`max_attempts` is total server-created execution-attempt commands, including the
initial `session.start`. Attempt count starts at zero before assignment and
increments exactly once in the same transaction that commits a new durable
`session.start` or `session.resume` command. It SHALL survive server and daemon
restarts and SHALL NOT increment for reconnect, command/event replay, heartbeat,
reconciliation, ACK retry, cancellation, or local runtime recovery.

A known failed attempt with remaining budget, a non-cancelled run, and a valid
pinned owner SHALL atomically close/clear the current attempt, set the logical
run to `Retrying`, persist `next_retry_at`, keep the clarification run
non-terminal and slot-occupying, and later create one explicit `session.resume`.
Due scheduling SHALL require no current attempt. A cancellation request or
invalid owner SHALL follow terminal cancellation/`owner_unavailable` policy
instead of scheduling a resume. Exhaustion SHALL set logical execution to
`Failed`, release the slot, and leave
Requirement lifecycle/content/revision/readiness unchanged. Once
`execution_outcome_unknown` terminalizes a logical run, that run SHALL never
receive `session.resume`; later execution requires a new logical run, new
`run_id`/protocol `session_id`, current context, a new `session.start`, and
normal sequential-slot/state-version rules.

#### Scenario: Initial command counts once

- **WHEN** the server commits the first durable `session.start` for a run
- **THEN** attempt count becomes 1 atomically with that command, and replay or
  retransmission of the command does not change it

#### Scenario: Known failure remains in the slot

- **WHEN** a current attempt emits a well-formed known failure and attempts remain
- **THEN** the server commits `Retrying` plus a persisted due time and safe
  reason, keeps phase `active`, and does not create a second run

#### Scenario: Exhaustion is server-decided

- **WHEN** a known failure leaves no attempt budget
- **THEN** the server commits terminal `Failed`, releases the clarification slot,
  and leaves Requirement status, revision, readiness, and state version unchanged

#### Scenario: Unknown outcome is not resubmitted

- **WHEN** a daemon reports `execution_outcome_unknown`
- **THEN** the server records safe terminal failure, creates no automatic
  `session.resume`, and permanently disallows `session.resume` for that logical
  run; later execution must use a new run and `session.start`

#### Scenario: Daemon restart does not reset attempts

- **WHEN** a daemon restarts after two server-directed attempts
- **THEN** the server still reports attempt count 2 and applies the remaining
  persisted policy budget

#### Scenario: Exhaustion is decided by server

- **WHEN** recoverable failure facts exhaust the persisted server retry budget
- **THEN** the server changes execution state to `Failed` and records a safe
  reason without changing Requirement lifecycle state

### Requirement: Daemon owns only transport and local recovery mechanics

The daemon MAY reconnect its WebSocket with backoff, replay buffered events,
reattach a local runtime transport when instructed, and report recoverability or
failure facts. `session.failed.recoverable` describes only daemon-local ability
to resume/reattach the existing operation; either value remains an input to
server policy, not authority. `session.failed` is an execution-attempt fact.
The daemon MUST NOT own a business retry budget, create a business retry, decide
permanent execution failure, migrate a pinned run, or mutate Requirement
lifecycle state. For `execution_outcome_unknown`, automatic daemon resubmission
is forbidden and the terminal logical run can never receive `session.resume`;
any later attempt uses a new logical run and explicit `session.start` with a new
command/run identity.

#### Scenario: Reconnect is delivery only

- **WHEN** a pinned daemon reconnects and replays an unacknowledged start/resume
  command
- **THEN** the server creates no new attempt and the command keeps its original
  identity/count

#### Scenario: Recoverability does not decide retry

- **WHEN** daemon reports `session.failed` with either recoverable value
- **THEN** server policy decides retry versus terminal failure; the daemon value
  alone cannot force either outcome

#### Scenario: Socket backoff is not a business attempt

- **WHEN** the daemon performs five WebSocket reconnects before one successful
  resume
- **THEN** the server attempt count increases only for the server-directed
  resume attempt

#### Scenario: Failure fact leaves business state alone

- **WHEN** the daemon reports a non-recoverable runtime failure
- **THEN** the server applies its own execution policy and Requirement status and
  revision remain unchanged

## ADDED Requirements

### Requirement: Failure facts are durably idempotent

The server SHALL bind each accepted `session.failed` to its logical run/current
attempt and process event identity, sequence, state transition, and retry
scheduling atomically. Duplicate/replayed failure facts SHALL return their
recorded ACK/outcome without decrementing budget, incrementing attempts,
creating another resume, or repeating terminalization. Same-sequence identity
conflicts retain protocol-error behavior.

#### Scenario: Duplicate failure cannot double-schedule

- **WHEN** the same failure event is delivered, replayed, or handled by two
  concurrent workers
- **THEN** one policy result commits and every duplicate observes that result with
  no second due row effect or resume command

### Requirement: Retry scheduling survives restart and concurrency

The server SHALL persist `next_retry_at` and discover due `Retrying` runs from
the database after startup and during bounded polling/wakeup. A due worker SHALL
lock/claim the session, verify cancellation/state/budget and
`current_attempt_id IS NULL`, then atomically create one pinned `session.resume`,
attempt row, new current-attempt identity, and attempt-count update. Database
uniqueness and row locking SHALL make concurrent workers and stale timers
harmless.

Owner validity is separate from owner liveness. A valid but offline pinned owner
MAY receive a durable outbox command for reconnect delivery; reconnect only
delivers it. An invalid/revoked owner cannot receive new retry work, is never
replaced by automatic migration, and follows terminal `owner_unavailable`
policy.

#### Scenario: Restart finds due retry

- **WHEN** server restarts after a failure committed `Retrying` and a due time
  passes
- **THEN** startup discovery creates at most one eligible resume using the
  persisted due time and no in-memory timer is required

#### Scenario: Due worker race is idempotent

- **WHEN** two retry workers or a worker and cancellation race for one due run
- **THEN** row locking yields one committed winner; the loser creates no second
  resume and cannot resurrect terminal cancellation

#### Scenario: Pinned owner validity and liveness are distinct

- **WHEN** a due retry's owner is valid but offline
- **THEN** server keeps ownership, queues one durable resume for reconnect, and
  reconnect delivery creates no new attempt

#### Scenario: Invalid pinned owner is not migrated

- **WHEN** a due retry's owner is revoked or its registration is permanently
  invalid
- **THEN** server creates no new retry work, applies terminal
  `owner_unavailable` policy, and never selects another daemon

### Requirement: Cancellation prevents future execution attempts

Cancellation SHALL remain run-scoped and explicit. For a running attempt it
shall persist intent and use/reuse one pinned `session.cancel`, with no later
retry after the terminal fact. For a retrying run with no current resume command,
cancellation shall atomically clear due work and terminalize the run as a safe
cancelled failure result. Waiting timers, stale retry workers, unavailable
owners, and reconnects SHALL not create or resurrect a resume. Cancellation
shall not affect a newer run with another `run_id`.

#### Scenario: Cancel while waiting for due time

- **WHEN** a retrying run is cancelled before its due resume is created
- **THEN** the run becomes terminal, its due schedule is cleared, the slot is
  released, and a stale worker creates no command

#### Scenario: Cancelled running attempt cannot retry

- **WHEN** cancellation is requested for a run with a current attempt and that
  attempt later fails
- **THEN** the server records terminal cancellation/failure and creates no
  `session.resume`

### Requirement: Public clarification projection exposes safe retry state

The existing `/requirements/{id}/session` read SHALL remain the only public run
projection. It SHALL retain `phase` values `awaiting_assignment`, `active`, and
`terminal`, and extend coarse `status` with `retrying` and `failed` only as
server projections. A policy-selected retry maps to
`phase=active,status=retrying`; exhausted, unknown-outcome, owner-unavailable,
or terminal cancellation maps to `phase=terminal,status=failed`. The projection
MAY include non-sensitive `attempt_count`, `next_retry_at`, and bounded safe
`failure_reason`; it SHALL NOT expose retry limits, remaining budget, daemon or
provider details, raw runtime errors, credentials, or operation IDs. Browser
clients SHALL not infer retry truth from transcript/activity and SHALL not
automatically resubmit.

#### Scenario: Retry is visibly active

- **WHEN** server policy schedules a retry
- **THEN** session read shows active/retrying, retains slot ownership, and
  exposes only safe retry fields

#### Scenario: Terminal failure is distinct from unavailable health

- **WHEN** retry policy exhausts or rejects an unknown outcome
- **THEN** session read shows terminal/failed, releases the slot, and Requirement
  lifecycle remains unchanged
