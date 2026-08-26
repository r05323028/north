## Purpose

Defines the single wire contract between server and daemon: explicit typed
messages, durable bidirectional delivery, directional ordering, compatibility,
and reconnect/resume without duplicate effects.

## ADDED Requirements

### Requirement: Explicit envelope with stable identifiers and disjoint directions

Every message SHALL carry a stable id, a session id, a type tag, a payload, and
`schema_version`. Server commands SHALL additionally carry `command_id` and
`server_command_seq`; daemon events SHALL additionally carry `event_id` and
`daemon_event_seq`. Messages belong to disjoint connection/control, server
command, or daemon event groups. `session.resume` is a server command only.
Command/event/control families SHALL be exhaustive enums in the shared wire
crate, which SHALL NOT depend on business crates.

#### Scenario: Wire crate purity

- **WHEN** architecture tests inspect `north-protocol` dependencies
- **THEN** no business or host crate edge exists

### Requirement: Server commands cross a durable daemon idempotency boundary

The server SHALL persist every command before dispatch and retry it with the
same id and sequence until the daemon sends `command_ack(status=accepted)`. The daemon
SHALL persist a command inbox/processed record before that ACK. The ACK means
durable receipt for processing, not runtime completion. A duplicate command
SHALL repeat its known ACK without invoking the runtime twice or duplicating a
side effect; one `message.send` id SHALL submit one user message at most once.

#### Scenario: Command retry is harmless

- **WHEN** reconnect delivers the same `message.send` command three times
- **THEN** the daemon durably accepts it once, submits it at most once, and acknowledges the duplicates without another runtime submission

### Requirement: Events are replayed and acknowledged after durable handling

The daemon SHALL journal unacknowledged events before transmission and replay
them in `daemon_event_seq` order. The server SHALL process each event id at
most once inside the transaction that commits its business effect or a durable
rejection record. `event_ack(status=accepted)` or `event_ack(status=rejected)` SHALL be sent only after
that commit; a rollback produces no success ACK. The daemon MAY compact
processed command records only to a durable contiguous per-session watermark
that remains for the session.

#### Scenario: Assessment ACK follows commit

- **WHEN** a valid `requirement.assessed` event is received
- **THEN** evidence and any lifecycle transition commit before `event_ack(status=accepted)`, and a duplicate produces no second promotion

#### Scenario: Stale fact is handled without promotion

- **WHEN** an otherwise well-formed assessment targets an older Requirement revision
- **THEN** the server commits a durable rejection/dedupe record, sends `event_ack(status=rejected)`, and leaves Requirement state unchanged

### Requirement: Directional sequences detect gaps and preserve order

`server_command_seq` and `daemon_event_seq` SHALL be monotonic independently
within each session and direction. Reconciliation SHALL carry
`ack_through_seq` and MAY carry sparse acknowledgements. A valid out-of-order
frame MAY be buffered but SHALL NOT affect business state until its gap closes.
A duplicate id+sequence SHALL be harmless and re-acknowledged; the same sequence
with a different id SHALL be a protocol error; a late acknowledged frame SHALL
be inert.

#### Scenario: Missing event blocks application

- **WHEN** event sequence 4 arrives before sequence 3
- **THEN** sequence 4 is buffered or causes replay reconciliation, and no sequence-4 business effect occurs before sequence 3 is handled

### Requirement: Protocol compatibility fails closed

Hello/registration and welcome SHALL carry exact `protocol_version: "0.1"`;
0.1.x frames SHALL use `schema_version: 1`. An incompatible peer, unknown
command/event type, or unsupported schema SHALL receive `protocol.error`, cause
no side effect, and have its connection closed. Unacknowledged durable frames
remain eligible for replay; peers SHALL NOT silently reinterpret unknown
payloads.

#### Scenario: Unknown frame cannot mutate state

- **WHEN** a compatible peer sends an unknown type or unsupported schema
- **THEN** the receiver sends a protocol error, performs no effect, closes the connection, and retains unacknowledged durable work

### Requirement: Session start carries server-assembled runtime context

`session.start` SHALL carry a complete `RequirementContext`, a bounded/relevant
`ConversationContext` excerpt, and enabled `RepositoryContext` metadata. The
DTOs SHALL contain no credentials, tokens, checkout paths, persistence handles,
or `north-domain` types. `session.resume` SHALL carry no daemon-event cursor;
transport replay state SHALL remain in reconciliation watermarks.

#### Scenario: Context is assembled before dispatch

- **WHEN** the server dispatches `session.start`
- **THEN** the daemon receives requirement fields, recent/relevant conversation messages, and enabled repository id/name/url/description metadata without database access

#### Scenario: Resume does not duplicate transport replay

- **WHEN** the server sends `session.resume`
- **THEN** its payload contains only execution-recovery data and no `daemon_event_seq` or event-stream cursor

### Requirement: Readiness assessment evidence is typed

`requirement.assessed` SHALL carry a typed readiness verdict, blockers,
assumptions, and reviewed repositories with non-empty repository ids and
commit SHAs. The wire crate SHALL validate structural fields without depending
on `north-domain`; server conversion SHALL apply domain policy separately.

#### Scenario: Invalid typed evidence is rejected structurally

- **WHEN** an assessment contains an empty blocker, repository id, commit SHA, or zero requirement revision
- **THEN** protocol validation rejects it before business coordination

### Requirement: WebSocket transport remains outside the North wire contract

North 0.1 SHALL use JSON text WebSocket messages. The server SHALL use an
Axum WebSocket upgrade/adapter and the daemon SHALL use a `tokio-tungstenite`
connection supervisor. Transport adapters SHALL stop at `north-protocol` frame
values and SHALL NOT expose Axum or Tungstenite message types from the protocol
crate. Binary messages, malformed WebSocket frames, and messages over configured
size limits SHALL be rejected without a business side effect.

#### Scenario: Text frame crosses the adapter boundary

- **WHEN** a valid `ServerFrame` or `DaemonFrame` is sent
- **THEN** the owning adapter serializes it as one JSON text WebSocket message and the peer decodes it back into the corresponding North frame

#### Scenario: Transport library does not imply North reliability

- **WHEN** a WebSocket reconnects or a ping/pong succeeds
- **THEN** North coordination still uses stable IDs, sequences, acknowledgements, journals/outbox, and reconciliation; it does not infer durable session state from transport liveness

#### Scenario: Binary or oversized input is received

- **WHEN** an adapter receives a binary message or a message/frame over its configured limit
- **THEN** it reports a transport/protocol error, performs no business mutation, and closes or rejects the connection according to the session coordinator policy
