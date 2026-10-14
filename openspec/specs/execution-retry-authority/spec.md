# execution-retry-authority Specification

## Purpose

Separates daemon transport recovery from server execution policy so attempt budgets, retry decisions, and terminal failure survive reconnects and daemon restarts.

## Requirements

### Requirement: Server owns execution state and retry policy

The system SHALL satisfy the complete normative contract in this scenario.

#### Scenario: Complete contract preserves all requirement clauses

- **WHEN** the described capability is exercised
- **THEN** the following clauses apply:
  For each logical run, server SHALL persist authoritative state (`Idle`, `Running`, `Retrying`, `Failed`; `Completed` remains success), attempt count, snapshotted `max_attempts`, due time, and safe failure reason. Only server policy may create `session.resume` after known failure, remaining budget, no cancellation, and valid pinned owner; daemon facts are inputs. Exhausted or unknown-outcome runs SHALL NOT resume; execution failure leaves Requirement lifecycle/content/revision/readiness unchanged.

#### Scenario: Attempt budget counts committed commands only

- **WHEN** the server commits a new durable `session.start` or `session.resume`
- **THEN** `max_attempts` counts total server-created execution-attempt commands including the initial start; count starts at zero before assignment and increments once in the same transaction. It survives server/daemon restarts and never increments for reconnect, command/event replay, heartbeat, reconciliation, ACK retry, cancellation, or local runtime recovery.

#### Scenario: Initial command counts once

- **WHEN** the server commits the first durable `session.start` for a run
- **THEN** attempt count becomes 1 atomically with that command, and replay or
  retransmission of the command does not change it

#### Scenario: Known failure remains in the slot

- **WHEN** a current attempt emits a well-formed known failure and attempts remain
- **THEN** the server atomically closes the current attempt and commits `Retrying` plus a persisted due time and safe reason, keeps phase `active`, and does not create a second run

#### Scenario: Exhaustion is server-decided

- **WHEN** a known failure leaves no attempt budget
- **THEN** the server commits terminal `Failed`, releases the clarification slot,
  and leaves Requirement status, revision, readiness, and state version unchanged

#### Scenario: Unknown outcome starts only a new logical run

- **WHEN** `execution_outcome_unknown` terminalizes a run
- **THEN** it never receives `session.resume`; later execution requires a new logical run, new `run_id`/protocol `session_id`, current context, and a new `session.start` under normal sequential-slot/state-version rules

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

The daemon MAY reconnect, replay buffered events, reattach only when instructed, and report local recoverability/failure facts; `session.failed` is an execution-attempt fact. Only server policy decides retries. It SHALL NOT own a budget, create retries, decide permanent failure, migrate pinned runs, or mutate Requirement state. For `execution_outcome_unknown`, never resubmit; later execution requires a new logical run, `session_id`/command identity, and explicit `session.start`.

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

Server SHALL persist `next_retry_at` and discover due `Retrying` runs after startup and by bounded poll/wakeup. A due worker SHALL verify cancellation/state/budget and no current attempt, then atomically create one pinned `session.resume`, attempt row/identity, and count increment; locking and uniqueness make races harmless.

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

Cancellation SHALL be run-scoped. For a running attempt, persist intent and use/reuse one pinned `session.cancel`; after its terminal fact, no retry. For a retrying run, atomically clear due work and terminalize cancelled failure. Timers, stale workers, unavailable owners, and reconnects SHALL NOT create/resurrect resume. Cancellation SHALL NOT affect a newer `run_id`.

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

Only `/requirements/{id}/session` SHALL project runs; phases are `awaiting_assignment/active/terminal`, statuses add `retrying/failed`. Retry maps active/retrying; exhaustion, unknown outcome, owner-unavailable, or cancellation maps terminal/failed. It MAY expose safe attempt count, due time, and bounded safe failure reason, but SHALL hide limits/budget, daemon/provider details, raw errors, credentials, and operation IDs. Browsers SHALL use it, not transcript/activity, and never auto-resubmit.

#### Scenario: Retry is visibly active

- **WHEN** server policy schedules a retry
- **THEN** session read shows active/retrying, retains slot ownership, and
  exposes only safe retry fields

#### Scenario: Terminal failure is distinct from unavailable health

- **WHEN** retry policy exhausts or rejects an unknown outcome
- **THEN** session read shows terminal/failed, releases the slot, and Requirement
  lifecycle remains unchanged
