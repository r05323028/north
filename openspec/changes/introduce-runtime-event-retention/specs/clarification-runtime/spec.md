## Purpose

Keeps runtime activity telemetry ephemeral under bounded retention without
weakening durable coordination state.

## MODIFIED Requirements

### Requirement: Runtime events project canonically after durable handling

For well-formed, session-bound runtime events, the server SHALL retain the
existing event identity/sequence checks, apply one idempotent projection, and
send the terminal event ACK only after that projection commits:

- `session.started` retains `phase=active` and sets coarse session status to `running`;
- `agent.message` appends one persisted `agent` conversation message;
- `agent.activity` appends one coarse product-visible activity record;
- `session.completed` sets `phase=terminal` and coarse status to `completed`
  without changing the Requirement; and
- `session.failed` sets `phase=terminal` and coarse status to `unavailable` as
  an operational fact without choosing retry or mutating the Requirement.

For an assigned run with `cancel_requested=true`, `session.completed` and
`session.failed` are the only existing runtime facts that close the run. A
`command_ack` for `session.cancel` is not a runtime fact and never changes
`phase`. A matching duplicate/replay SHALL return the known ACK without
repeating the projection. A different payload or identity reuse remains a
protocol conflict. Raw tool output and chain-of-thought SHALL never enter
message/activity read models.

Coarse `agent.activity` telemetry is ephemeral. Each accepted activity row
carries an expiry written from the configured retention window and may be
removed by bounded retention sweeps. Durable coordination state — event
dedupe/outcome records, ACK watermarks and sparse ACK lists, execution
attempts, retry scheduling, and failure classification — is never a retention
target. Replaying an already-acknowledged activity event remains deduplicated
and does not recreate purged telemetry.

#### Scenario: Agent message becomes canonical history

- **WHEN** a valid `agent.message` event is committed
- **THEN** the existing conversation HTTP read returns it, and a duplicate event does not add a second message

#### Scenario: Completion does not mean Ready

- **WHEN** `session.completed` arrives with no accepted assessment
- **THEN** the session reads completed, the Requirement remains unchanged, and no synthetic readiness result is created

#### Scenario: Expired activity stays deduplicated

- **WHEN** an accepted `agent.activity` event is replayed after its telemetry row expired and was purged
- **THEN** the server returns the recorded duplicate outcome without reinserting a row or changing watermarks or dedupe state
