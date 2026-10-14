# daemon-protocol Specification

## Purpose

Defines the single wire contract between server and daemon: explicit typed
messages, durable bidirectional delivery, directional ordering, compatibility,
and reconnect/resume without duplicate effects.

## Requirements

### Requirement: Envelopes and connection frames have explicit fields

Delivery envelopes SHALL be only `CommandEnvelope` and `EventEnvelope`. Both SHALL carry `session_id`, `sent_at`, and `schema_version`; commands also carry `command_id`, `server_command_seq`, and one typed command; events carry `event_id`, `daemon_event_seq`, and one typed event. Connection/control frames SHALL use distinct schemas and SHALL NOT require `session_id`. ACKs MAY copy target session/sequence but are not envelopes. `session.resume` SHALL be server-command-only.

#### Scenario: Connection frame has no envelope requirement

- **WHEN** a `hello`, `welcome`, `heartbeat`, `ReconcileSnapshot`, or
  `protocol.error` frame is validated
- **THEN** validation checks that frame's defined connection-level fields and
  does not require command/event envelope fields or `session_id`

#### Scenario: Directional envelope fields are complete

- **WHEN** a command or event is serialized
- **THEN** its stable ID, session ID, direction-specific sequence, timestamp,
  schema version, and typed payload round-trip without an implicit alternate
  envelope shape

### Requirement: Command and event identities and sequences have defined scope

`command_id` and `event_id` SHALL be globally unique opaque IDs in separate namespaces. Server outbox/daemon command journal SHALL enforce command-ID uniqueness; daemon event journal/server dedupe SHALL enforce event-ID uniqueness. `server_command_seq` (server→daemon) and `daemon_event_seq` (daemon→server) SHALL be unique, monotonic per session, start at 1, and persist with the durable message.

#### Scenario: Same global ID cannot map to two deliveries

- **WHEN** a durable store receives an already-used command or event ID with a
  different session, sequence, type, or payload digest
- **THEN** it rejects the frame as a protocol/integrity error without a
  business effect or acknowledgement for the conflicting delivery

### Requirement: Sequence allocation and durable records commit atomically

`server_command_seq` allocation and complete command-outbox insertion SHALL be one durable transaction; `daemon_event_seq` allocation and complete event-journal append SHALL be one durable journal operation. A sequence SHALL NOT enter durable session space without its message record. Pre-commit failure SHALL leave that next value available. In-memory counters or non-transactional allocation SHALL NOT define order.

#### Scenario: Failed command transaction leaves no hole

- **WHEN** command sequence allocation or outbox insertion rolls back
- **THEN** no durable record or committed sequence value exists for that
  attempt and the next successful command can use the same next sequence

#### Scenario: Failed event append leaves no hole

- **WHEN** a daemon event journal append fails before its durable commit
- **THEN** the event sequence is absent from the durable session sequence space
  and later allocation can reuse that next sequence

### Requirement: Server command delivery is durably outboxed and acknowledged

The server SHALL persist the complete immutable command envelope before
attempting dispatch. It SHALL retry an unacknowledged command with the original
`command_id`, `session_id`, `server_command_seq`, and payload, in ascending
sequence order. A daemon `command_ack` SHALL mean durable command receipt for
processing, not runtime completion. The server SHALL durably process ACK
identity and digest before advancing its contiguous command watermark.

#### Scenario: Reconnect resends original command

- **WHEN** a connection closes before a command ACK is durably recorded
- **THEN** the server resends the stored envelope with the same ID, sequence,
  session, and payload rather than allocating a replacement command

#### Scenario: ACK above a gap does not create a contiguous watermark

- **WHEN** command sequence 5 is durably ACKed but sequence 4 is not
- **THEN** the server retains sequence 4 for resend and does not advance
  `command_ack_through_seq` through sequence 5

### Requirement: Daemon command journal has explicit recovery states

Commands SHALL persist `received` (complete command+payload digest committed; `command_ack` MAY follow), `dispatch_started` (committed before a possibly duplicating runtime call), and `terminal` (durable local outcome; no automatic reinvocation). Terminal metadata SHALL include `dispatch_succeeded`/`dispatch_failed`/`unknown`, reason, timestamp, and stable runtime-operation ID. Implementations MAY label dispatch outcomes `completed`/`failed`; these are not execution-session completion.

#### Scenario: Received command survives daemon crash

- **WHEN** the daemon crashes after committing `received` and before dispatch
- **THEN** restart may continue that durable command and sends no second
  command identity

#### Scenario: Dispatch boundary is durable before invocation

- **WHEN** a side-effecting runtime operation is about to be invoked
- **THEN** `dispatch_started` is durably committed first with
  `runtime_operation_id` equal to the stable `command_id`

#### Scenario: Duplicate delivery is inert in every state

- **WHEN** a command is redelivered in any journal state
- **THEN** the daemon returns its known ACK/outcome where appropriate and SHALL NOT invoke the runtime twice

### Requirement: Durable command processing crosses a narrow runtime seam

Coordinator SHALL decide journal state/idempotency before crossing a narrow runtime seam. The seam SHALL accept stable command/runtime identity (`runtime_operation_id = command_id` in 0.1.0); its Rust type/name is not wire contract. Replayed commands SHALL cross only when journal state permits; terminal/duplicate-safe records SHALL NOT cross again. This protocol SHALL NOT introduce prompting, SDK behavior/tool choice, repository inspection, or readiness judgment.

#### Scenario: Durable tests use a fake executor

- **WHEN** durable-delivery tests process a command without a real agent runtime
- **THEN** a deterministic fake crosses the seam with the stable operation
  identity and the journal assertions remain valid

#### Scenario: Duplicate-safe command does not cross seam twice

- **WHEN** a replayed command is already terminal or otherwise duplicate-safe
- **THEN** the coordinator returns its known result without another seam call

#### Scenario: Clarification adapter remains behind the seam

- **WHEN** a real clarification command is dispatched
- **THEN** the landed `clarification-runtime` capability provides the agent-runtime adapter behind this same seam

### Requirement: Unknown dispatch outcome is explicit and non-duplicating

After crash post-`dispatch_started`, restart SHALL first reattach/status-check by stable operation ID. `session.failed.recoverable` reports only local safe reattach/resume ability; it neither forbids server-directed attempts nor decides execution failure. Unknown outcome SHALL persist terminal `unknown` and journal `session.failed`; daemon SHALL NOT resubmit automatically. Later `session.resume`/`session.start` requires explicit server direction, new command identity, and server retry policy.

#### Scenario: Crash after dispatch does not double-submit

- **WHEN** the daemon crashes after the runtime call and cannot determine whether it was received, running, complete, or failed
- **THEN** restart records terminal unknown as an execution fact (not a protocol error), journals `session.failed` with `recoverable: false` and a reason containing `execution_outcome_unknown`, `command_id`, `runtime_operation_id`, and `automatic_resubmit=false`, and does not invoke the side-effecting operation again

#### Scenario: Reattachment resolves outcome

- **WHEN** runtime reattachment by stable operation identity proves completion
  or failure
- **THEN** the daemon records that terminal outcome without invoking a second
  operation

### Requirement: `message.send` has separate transport and logical identity

For `message.send`, `command_id` identifies transport; `message_id` identifies the logical message. Server SHALL create one immutable command mapping per message; retries SHALL reuse command ID, message ID, and content. Daemon SHALL enforce one `(session_id,message_id)`→command ID with immutable content; same-message/different-command is an integrity/protocol rejection with no runtime submission. One logical message SHALL cause at most one automatic runtime submission across reconnect/restart.

#### Scenario: Duplicate message command is harmless

- **WHEN** the same `message.send` command is replayed after reconnect or daemon
  restart
- **THEN** its original `message_id` and content are recognized, the known ACK
  is returned, and runtime submission occurs at most once

#### Scenario: Message identity mismatch is rejected

- **WHEN** the same `message_id` arrives under a different command ID or with
  different content
- **THEN** the daemon records no second runtime submission and emits a terminal
  protocol/integrity error

### Requirement: Events are journaled before transmission and ACKed after handling

Daemon SHALL atomically allocate `daemon_event_seq` and append each event before transmission. Server SHALL process each event at most once in a transaction validating identity/payload, committing effect or durable rejection, and recording dedupe. `event_ack(status=accepted)` SHALL follow effect commit; `event_ack(status=rejected)` SHALL follow rejection commit. Both are terminal for that identity; rejection does not retry. Rollback, protocol error, or no ACK leaves replay eligible.

#### Scenario: Assessment ACK follows the transaction

- **WHEN** a valid current-revision `requirement.assessed` event is received
- **THEN** typed evidence and any valid Requirement transition commit before
  `event_ack(status=accepted)`, and a duplicate cannot promote twice

#### Scenario: Stale assessment is a durable rejection

- **WHEN** a well-formed assessment targets an older Requirement `revision`
- **THEN** the server records durable rejection/dedupe state, sends
  `event_ack(status=rejected)`, and leaves Requirement state unchanged

#### Scenario: Rejected ACK is terminal

- **WHEN** a daemon receives a durable `event_ack(status=rejected)` for an event
- **THEN** it removes/compacts that event payload after reconciliation state is
  durable and does not replay or retry the business effect

### Requirement: Reconciliation merges command and event delivery state

After authentication and before application readiness, server SHALL send one finite connection-level `ReconcileSnapshot` with at most one unique entry per pinned session (possibly zero). `command_ack_through_seq` covers only commands durably known by daemon via server-recorded ACKs; `event_ack_through_seq` covers events durably handled by server; `event_ack_sparse` lists handled higher sequences.

#### Scenario: Snapshot handles no pinned sessions

- **WHEN** an authenticated daemon owns no execution sessions
- **THEN** the server sends one valid snapshot with an empty `sessions` list

#### Scenario: Late command is inert

- **WHEN** a command at or below the daemon's durable processed/accepted
  watermark arrives with matching retained identity
- **THEN** the daemon never invokes runtime and returns the known
  `command_ack` where appropriate

#### Scenario: Sparse event ACK suppresses replay

- **WHEN** event sequence 7 appears in durable `event_ack_sparse` while the event watermark is 4
- **THEN** event 7 is not replayed; unacknowledged event 5 or 6 remains eligible, and journal replay is ascending by `daemon_event_seq`

#### Scenario: Unacknowledged command resend preserves identity and order

- **GIVEN** `command_ack_through_seq` confirms through N and identity/tombstone state is retained
- **WHEN** the server compacts payloads at/below N while commands above N lack durable ACK
- **THEN** it MAY compact confirmed payloads only after retaining identity/tombstones; above-watermark commands remain resend-eligible with original ID/sequence/payload in ascending order

### Requirement: Sequence gaps are bounded and business-inert

For each session/direction, valid frames above expected sequence MAY be durably buffered but SHALL NOT affect business state before gap closure. North 0.1 SHALL bound pending durable+memory frames with configurable `max_gap_buffer_entries_per_session` (default 256). On exhaustion, receiver SHALL withhold ACK/application and close at a retryable reconciliation boundary; it SHALL NOT require unbounded memory or treat a valid gap as `protocol.error`.

#### Scenario: Out-of-order event cannot mutate state

- **WHEN** event sequence 4 arrives before sequence 3
- **THEN** sequence 4 is bounded-buffered or reconciliation is initiated, and no
  sequence-4 business effect occurs before sequence 3 is handled

#### Scenario: Conflicting sequence closes safely

- **WHEN** frames reuse session sequence/identity inconsistently
- **THEN** same sequence+ID+payload is an inert duplicate/re-ACK; same sequence/different ID, same ID/different sequence, or same ID+sequence/different payload is a protocol/integrity error; the receiver performs no business mutation, sends a directional `protocol.error`, and closes the connection

#### Scenario: Gap-buffer exhaustion is retryable and bounded

- **GIVEN** `max_gap_buffer_entries_per_session` is full (default 256)
- **WHEN** another valid out-of-order frame arrives
- **THEN** the receiver withholds ACK and business application and closes at a retryable reconciliation boundary; it neither grows unbounded nor labels the valid gap `protocol.error`

### Requirement: Compaction retains durable duplicate protection

Compaction SHALL retain per-session watermarks and compact identity/payload tombstones sufficient for inert duplicates and visible conflicts. `processed_through_seq` SHALL NOT expire solely by time while durable session identity remains relevant. Server SHALL retain event dedupe so late replay cannot reapply business effects. Raw payload MAY be removed once compact identity, sequence, digest, and outcome protection is durable.

#### Scenario: Old command remains duplicate-safe after payload compaction

- **WHEN** a compacted command below `processed_through_seq` is replayed with
  its original identity
- **THEN** the daemon returns an inert known result and does not need the full
  payload or invoke runtime

#### Scenario: Event dedupe survives payload removal

- **WHEN** an acknowledged event payload is removed and a late identical event
  is replayed
- **THEN** the server's retained identity/watermark/digest state prevents a
  second business effect and returns the known terminal ACK

#### Scenario: Payload compaction follows durable delivery

- **WHEN** a durable delivery payload is compacted
- **THEN** server command payload follows daemon ACK plus contiguous command watermark; daemon command payload follows terminal processing plus contiguous processed watermark; daemon event payload follows durable accepted/rejected ACK
- **AND** watermarks and compact identity/sequence/digest/outcome tombstones remain sufficient for inert duplicates and visible conflicts; raw payload MAY be removed after that protection is durable

### Requirement: Execution terminal state is separate from Requirement lifecycle

Transport cleanup SHALL follow execution-session delivery state and durable ACK
watermarks, not Requirement `Accepted` or `Rejected` state. A terminal
execution session MAY still retain unacknowledged delivery work. A Requirement
may be Accepted while its execution session still has replay work. Protocol
delivery SHALL not alter the server's authoritative `revision`,
`state_version`, `assessment_id`, or `accepted_state_version` semantics.

#### Scenario: Accepted Requirement does not discard pending event

- **WHEN** a Requirement becomes Accepted while its execution session has an
  unacknowledged event
- **THEN** the event remains journaled and replay-eligible until its durable
  event ACK boundary is reached

### Requirement: Protocol compatibility and errors fail closed in both directions

Hello/welcome SHALL use `protocol_version:"0.1"`; 0.1.x SHALL use `schema_version:1`. `ServerFrame`/`DaemonFrame` SHALL define connection/control `protocol.error` for peer violations. Unknown type, unsupported schema/version, invalid identity reuse, or incompatibility SHALL cause no business effect, send error if encodable, and close connection. Errors SHALL have no severity discriminator; receipt ends connection; reconnect is host policy. Unacknowledged durable work remains replay-eligible.

#### Scenario: Daemon reports invalid server command

- **WHEN** the daemon detects an unknown server command, unsupported schema, or
  identity/payload conflict
- **THEN** it sends `DaemonFrame::ProtocolError`, performs no business/runtime
  effect, and closes the current connection

#### Scenario: Server reports invalid daemon event

- **WHEN** the server detects an invalid daemon event or sequence/identity
  conflict
- **THEN** it sends `ServerFrame::ProtocolError`, performs no business effect,
  and closes the current connection

### Requirement: Handshake and transport boundaries remain unchanged

North 0.1 SHALL use JSON-text WebSocket with Axum server and tokio-tungstenite daemon. One supervisor SHALL own hello, split reader/writer tasks, ping/pong, bounded queues, disconnect/backoff, and reconnect. It SHALL expose Welcome and complete ReconcileSnapshot to coordination, gating Active, heartbeat, events, ACKs, replay, and commands until coordination is ready; ping/pong remains available pre-Active. Transport liveness SHALL NOT imply durable delivery or business completion.

#### Scenario: Snapshot receipt does not activate traffic

- **WHEN** a valid snapshot arrives but coordination has not applied it and signaled ready
- **THEN** application traffic remains gated and the daemon is not `Active`; WebSocket ping/pong remains available

#### Scenario: Handshake stages have bounded retryable timeouts

- **WHEN** hello, welcome/authentication, or reconciliation exceeds its configured stage timeout, or coordination exceeds its total readiness budget
- **THEN** handshake/reconciliation timeouts are configurable and distinct from execution retry; coordination uses one total budget for `HandshakeComplete` delivery, snapshot application, and readiness signaling; timeout is retryable

### Requirement: Context and readiness authority remain server-owned

The server SHALL assemble complete safe `session.start` context, validate typed assessments/readiness, and own Requirement state; the daemon reports facts only and SHALL NOT mutate Requirement state. The wire layer SHALL validate only structurally non-empty `repository_id`/`commit_sha`, without repository persistence. `session.resume` SHALL carry execution recovery only, never a transport event cursor.

#### Scenario: Delivered assessment cannot bypass server gates

- **GIVEN** `requirement.assessed` carries typed evidence for a uniquely identified, session-bound, sequence-valid event
- **WHEN** event identity/`daemon_event_seq` are validated, `requirement_revision` matches the current Requirement revision, and server/domain readiness gates pass
- **THEN** the server atomically records immutable evidence, MAY promote `Discussing` to `Ready`, records the resulting Ready-generation `state_version` as `accepted_state_version`, and binds `assessment_id` and `accepted_state_version`
- **AND** these values are not inbound assessment concurrency tokens; stale revision or failed gates are durably rejected under domain transaction rules without changing Requirement status, revision, or `state_version`

#### Scenario: Resume does not own replay state

- **WHEN** a `session.resume` command is encoded
- **THEN** it contains execution-recovery data only and no transport event cursor; directional sequence/replay watermarks remain in reconciliation state

#### Scenario: Session start contains complete server context

- **WHEN** the server creates `session.start`
- **THEN** it includes complete server-assembled Requirement context, bounded relevant conversation, and enabled repository metadata, without credentials, checkout paths, persistence handles, or domain types

#### Scenario: Human review targets the accepted Ready generation

- **WHEN** a human later Accepts, Rejects, or Requests Changes
- **THEN** each decision uses `assessment_id`, `expected_state_version`, and the exact current Ready generation

#### Scenario: Wire repository validation remains structural

- **WHEN** `north-protocol` validates repository metadata
- **THEN** it checks only structurally non-empty `repository_id`/`commit_sha` and does not access repository persistence

### Requirement: Runtime events use owning clarification projection

This change SHALL alter only clarification-owned `session.failed`: other events keep owning projection and identity/sequence/payload/dedupe/ACK-after-commit routing; generic rejection is unchanged. Failure becomes an execution-attempt fact governed by execution-retry-authority, not unconditional run termination. Requirement lifecycle/content/revision/readiness and transport replay/watermarks remain unchanged; replayed facts are inert. Owner liveness alone SHALL NOT decide policy.

#### Scenario: Clarification event reaches its owning projection

- **WHEN** a valid `session.started`, `agent.message`, `agent.activity`, or
  `session.completed` event targets an assigned clarification session
- **THEN** server SHALL retain the landed owning projection and ACK-after-commit
  behavior; this retry delta adds no generic-event projection behavior

#### Scenario: Current failure projection is terminal

- **WHEN** `session.failed` is exhausted, unknown-outcome, owner-invalid, or
  terminally cancelled
- **THEN** server SHALL project `phase=terminal,status=failed`, release the
  sequential slot, and reject `session.resume` for that logical run

#### Scenario: Unsupported ownership uses generic rejection

- **WHEN** a well-formed runtime event has no owning clarification projection
- **THEN** server MAY record `event_handler_not_implemented` durably and SHALL
  send a rejected ACK only after that rejection commits

#### Scenario: Failure event changes from terminal run fact to attempt fact

- **WHEN** a valid clarification-owned `session.failed` fact arrives for the
  current execution attempt
- **THEN** the server SHALL record the attempt outcome and clear the current
  attempt before applying retry policy, rather than unconditionally
  terminalizing the logical run

#### Scenario: Known failure remains retrying

- **WHEN** failure is known, the logical run is not cancelled, the pinned owner
  is valid, and retry budget remains
- **THEN** server SHALL persist safe failure state and `next_retry_at`, leave the
  run active/retrying and slot-occupying, and later create one explicit
  `session.resume`

#### Scenario: Unknown outcome is terminal and not resumable

- **WHEN** local reattachment fails and the daemon reports
  `execution_outcome_unknown`
- **THEN** server SHALL terminalize the logical run as failed, create no
  automatic `session.resume`, and reject any later resume for that terminal run;
  a later execution requires a new logical run and explicit `session.start`

### Requirement: Runtime and business retry ownership stays separated

The daemon SHALL own only transport reconnect/backoff, local journal recovery,
and runtime reattachment by stable operation identity. The server SHALL own
business retry/failure policy, session ownership, Requirement lifecycle,
readiness decisions, and repository authorization. The protocol SHALL not
claim exactly-once runtime execution or move agent prompting, tool choice,
repository inspection, or business retry into the wire crate.

#### Scenario: Transport retry does not become business retry

- **WHEN** a socket reconnects after a command or event fault
- **THEN** durable IDs, sequences, journals, ACKs, and server policy determine
  behavior; transport alone does not submit a second runtime operation or
  mutate Requirement state
