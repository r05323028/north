# clarification-runtime Specification Delta

## MODIFIED Requirements

### Requirement: Runtime events project canonically after durable handling

For well-formed session-bound runtime events, North SHALL retain existing event
identity/sequence validation and ACK only after one idempotent server
projection commits. `session.failed` is an execution-attempt fact, not an
unconditionally terminal logical-run fact. A known failure with retry policy
budget remaining clears/closes the failed current attempt, persists safe failure
classification and `next_retry_at`, and leaves the logical run
`phase=active,status=retrying`. Exhaustion, `execution_outcome_unknown`,
owner invalidation, or cancellation terminalizes the logical run as
`phase=terminal,status=failed`. A retrying run retains the sequential slot; a
terminal run releases it. Raw runtime/provider details remain private.

`session.completed` remains successful terminal completion. Duplicate/replayed
facts return their recorded ACK/outcome without repeating effects. A terminal
unknown-outcome run can never receive `session.resume`; later execution starts a
new logical run with a new run/protocol session identity and normal start/slot
rules.

#### Scenario: Agent message becomes canonical history

- **WHEN** a valid `agent.message` event commits
- **THEN** one canonical agent message is persisted and duplicate delivery does
  not add another message

#### Scenario: Completion does not mean Ready

- **WHEN** `session.completed` arrives without an accepted assessment
- **THEN** the run completes without changing Requirement lifecycle or creating
  synthetic readiness

#### Scenario: Retryable failure remains active

- **WHEN** a known current-attempt `session.failed` fact is accepted with retry
  budget remaining
- **THEN** the failed current attempt is closed, the run is active/retrying with
  durable due work, and no new logical run is created

#### Scenario: Unknown outcome forbids same-run resume

- **WHEN** `execution_outcome_unknown` terminalizes run A
- **THEN** A cannot receive `session.resume`; later execution requires new run B,
  new `run_id`/protocol `session_id`, and a new `session.start`

### Requirement: Completion and failure facts have explicit semantics

A normal completion SHALL remain a valid terminal session fact even without an
assessment and never changes Requirement content, lifecycle, revision, readiness,
or state version. A failure fact is processed once and then follows server
retry policy. Known retryable failure keeps the logical run active and
slot-occupying; retry exhaustion, unknown outcome, invalid owner, or terminal
cancellation failure
produces safe terminal failed state. Successful cancellation uses the existing
completion fact. The daemon `recoverable` value is a local fact only.

#### Scenario: Failure before assessment leaves business truth intact

- **WHEN** runtime fails before producing an assessment
- **THEN** server policy shows active/retrying or terminal/failed without changing
  Requirement status, revision, readiness, or state version

#### Scenario: Successful cancellation uses existing completion fact

- **WHEN** a requested runtime cancellation terminates successfully
- **THEN** the existing `session.completed` fact projects terminal/completed with
  cancellation intent and no new cancellation frame

#### Scenario: Assessment and completion replay safely

- **WHEN** assessment and completion facts replay with original identities
- **THEN** each committed effect applies once and duplicate ACK outcomes remain
  stable

### Requirement: Clarification runs occupy one sequential clarification slot

The derived slot SHALL remain occupied by `phase=awaiting_assignment` and every
assigned non-terminal run, including `phase=active,status=retrying`. A terminal
failed, completed, or explicitly cancelled run releases it. A retry worker may
create a new attempt only for the same non-terminal run while it is retry-eligible.
A terminal run, especially one terminalized by unknown outcome, is never resumed
and cannot block or mutate a newer run.

#### Scenario: No daemon still creates an awaiting run

- **WHEN** no eligible daemon exists for a new start
- **THEN** the unassigned awaiting run occupies the slot without an execution
  attempt

#### Scenario: Reuse unavailable start attempt

- **WHEN** the same start message retries an awaiting run
- **THEN** the server reuses that run and does not create another run or start
  command

#### Scenario: No concurrent run while active

- **WHEN** run A is active/retrying and another start message arrives
- **THEN** the request receives the existing-run conflict and creates no run or
  start command

#### Scenario: Assigned cancellation intent does not terminate execution

- **WHEN** cancellation is acknowledged for a running assigned attempt
- **THEN** the run remains active until its terminal runtime fact and no later
  failure can schedule retry after cancellation intent

#### Scenario: Terminal runtime fact releases cancelled run

- **WHEN** a cancelled running attempt reaches terminal completion/failure
- **THEN** the run becomes terminal, releases the slot, and a later start may
  create a new logical run

#### Scenario: Unassigned cancellation is immediately terminal

- **WHEN** an awaiting run with no start command is cancelled
- **THEN** it becomes terminal without daemon command or execution attempt

#### Scenario: New run after terminal completion

- **WHEN** run A is terminal and a new eligible message starts clarification
- **THEN** run B has a new identity and independent command/event sequence

#### Scenario: Retry-waiting cancellation releases slot

- **WHEN** a retrying run with no current attempt is cancelled before due resume
- **THEN** server clears due work, terminalizes the run safely, and a stale worker
  cannot create or resurrect it

### Requirement: Canonical read models are server-owned

The existing session read SHALL remain the only public run projection. It retains
`awaiting_assignment`/`active`/`terminal` phases and adds safe
`attempt_count`, nullable `next_retry_at`, and bounded `failure_reason` as
available. A policy retry reads `phase=active,status=retrying`; terminal
execution failure, cancellation, owner invalidation, or unknown outcome reads
`phase=terminal,status=failed`. The projection never exposes retry limits,
remaining budget, daemon/provider identity, raw runtime errors, credentials, or
operation IDs.

#### Scenario: Browser reads persisted agent output

- **WHEN** browser refetches conversation after reconnect
- **THEN** it receives persisted messages without reading daemon frames

#### Scenario: Current assessment is explicit

- **WHEN** an assessment targets an old revision or Ready generation
- **THEN** readiness read marks it historical/non-current

#### Scenario: Session read returns latest run semantics

- **WHEN** a requester reads after an awaiting, running, retrying, completed,
  failed, or cancelled run exists
- **THEN** the latest run projection is returned, while old run history remains
  server-owned and a new start uses a new run identity

#### Scenario: Retry projection retains the sequential slot

- **WHEN** a known execution failure is retryable and the server persists a due retry
- **THEN** the session read returns `phase=active`, `status=retrying`, safe retry
  fields only, and a new start cannot claim the Requirement slot

#### Scenario: Terminal failure releases the slot

- **WHEN** retry policy exhausts or rejects an unknown execution outcome
- **THEN** the session read returns `phase=terminal`, `status=failed`, the
  Requirement remains unchanged, and a later eligible start may create a new run

### Requirement: Cancellation distinguishes intent from completion

Cancellation SHALL remain explicit and run-scoped. Running attempts persist intent
and create/reuse one pinned `session.cancel`; later failure cannot schedule retry.
Retrying/waiting runs with no current attempt terminalize locally, clear due work,
and release the slot. Cancellation, worker, reconnect, and stale old-run
operations cannot resurrect a terminal run or affect a newer run.

#### Scenario: Unassigned cancellation is immediately terminal

- **WHEN** an unassigned run is cancelled
- **THEN** cancellation is persisted terminally without a daemon command

#### Scenario: Repeated unassigned cancellation

- **WHEN** the same unassigned run is cancelled repeatedly
- **THEN** persisted terminal state is reused idempotently

#### Scenario: Cancellation intent does not terminate assigned execution

- **WHEN** assigned cancellation receives `command_ack` but no runtime terminal
  fact exists
- **THEN** the run remains active and slot-occupying, with later dispatch blocked

#### Scenario: Terminal runtime fact releases cancelled run

- **WHEN** a cancelled assigned attempt emits terminal fact
- **THEN** the server closes that run according to completion/failure policy and
  releases its slot

#### Scenario: Successful cancellation uses existing completion fact

- **WHEN** runtime confirms successful cancellation
- **THEN** `session.completed` projects terminal/completed and preserves intent

#### Scenario: Stale cancellation cannot target a newer run

- **WHEN** delayed cancellation targets terminal run A after run B exists
- **THEN** only A is evaluated and B is untouched

#### Scenario: Unknown or cross-Requirement run is not found

- **WHEN** cancellation names an unknown or cross-Requirement run
- **THEN** server returns not-found and creates no command or state change
