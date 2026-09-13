## Purpose

Keeps runtime activity telemetry ephemeral under bounded retention without
weakening durable coordination state.

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

Coarse `agent.activity` telemetry is ephemeral. Each accepted activity row
carries an expiry written from the configured retention window and may be
removed by bounded retention sweeps. Durable coordination state — event
dedupe/outcome records, ACK watermarks and sparse ACK lists, execution
attempts, retry scheduling, and failure classification — is never a retention
target. Replaying an already-acknowledged activity event remains deduplicated
and does not recreate purged telemetry.

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

#### Scenario: Expired activity stays deduplicated

- **WHEN** an accepted `agent.activity` event is replayed after its telemetry row expired and was purged
- **THEN** the server returns the recorded duplicate outcome without reinserting a row or changing watermarks or dedupe state
