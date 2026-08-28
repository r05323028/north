# daemon-protocol Specification

## Purpose

Defines the single wire contract between server and daemon: explicit typed
messages, durable bidirectional delivery, directional ordering, compatibility,
and reconnect/resume without duplicate effects.

## ADDED Requirements

### Requirement: Envelopes and connection frames have explicit fields

Only `CommandEnvelope` and `EventEnvelope` are delivery envelopes. Every
`CommandEnvelope` SHALL carry `command_id`, `session_id`,
`server_command_seq`, `sent_at`, `schema_version`, and one typed command. Every
`EventEnvelope` SHALL carry `event_id`, `session_id`, `daemon_event_seq`,
`sent_at`, `schema_version`, and one typed event. Connection/control frames
SHALL use their own schemas and SHALL NOT be required to carry `session_id`.
ACK frames may copy the target session and sequence because they identify the
message being acknowledged; they are not envelopes. `session.resume` is a
server command only.

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

`command_id` SHALL be a globally unique opaque identity in the command
namespace. `event_id` SHALL be a globally unique opaque identity in the event
namespace. The server outbox and daemon command journal SHALL enforce unique
`command_id`; the daemon event journal and server event dedupe record SHALL
enforce unique `event_id`. `server_command_seq` SHALL be unique and monotonic
within one session in the server-to-daemon direction. `daemon_event_seq` SHALL
be unique and monotonic within one session in the daemon-to-server direction.
Both sequences SHALL start at 1 and SHALL be stored with their durable record.

#### Scenario: Same global ID cannot map to two deliveries

- **WHEN** a durable store receives an already-used command or event ID with a
  different session, sequence, type, or payload digest
- **THEN** it rejects the frame as a protocol/integrity error without a
  business effect or acknowledgement for the conflicting delivery

### Requirement: Sequence allocation and durable records commit atomically

For server commands, allocation of `server_command_seq` and insertion of the
complete command outbox record SHALL commit as one durable transaction. For
daemon events, allocation of `daemon_event_seq` and append/commit of the
complete event journal record SHALL commit as one durable journal operation. A
sequence value SHALL NOT enter the durable session sequence space unless its
message record also commits. A failure before commit SHALL allow that same next
sequence value to be allocated later. Best-effort in-memory counters and
non-transactional sequence allocation SHALL NOT define ordering.

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

Every command SHALL have durable state with these meanings: `received` means
the complete command and payload digest are committed and a `command_ack` MAY
be sent; `dispatch_started` means that state was committed before invoking a
runtime operation whose duplicate could matter; `terminal` means no further
automatic invocation is allowed and durable outcome metadata is present.
Terminal outcome metadata SHALL identify `completed`, `failed`, or `unknown`,
with reason, timestamp, and stable runtime operation identity. A duplicate in
any state SHALL return its known ACK where appropriate and SHALL NOT invoke the
runtime twice.

#### Scenario: Received command survives daemon crash

- **WHEN** the daemon crashes after committing `received` and before dispatch
- **THEN** restart may continue that durable command and sends no second
  command identity

#### Scenario: Dispatch boundary is durable before invocation

- **WHEN** a side-effecting runtime operation is about to be invoked
- **THEN** `dispatch_started` is durably committed first with
  `runtime_operation_id` equal to the stable `command_id`

#### Scenario: Terminal duplicate is inert

- **WHEN** a command in terminal state is delivered again
- **THEN** the daemon returns the known durable receipt/outcome information as
  applicable and never invokes the runtime again

### Requirement: Unknown dispatch outcome is explicit and non-duplicating

If the daemon crashes after durable `dispatch_started` and before terminal
outcome commit, restart SHALL first attempt runtime reattachment/status
recovery using the stable operation identity. It MUST NOT automatically
resubmit a side-effecting operation solely because outcome is unknown. If the
runtime result cannot be safely determined, the daemon SHALL persist terminal
`unknown` and emit a journaled `session.failed` event with `recoverable: false`
and a reason containing `execution_outcome_unknown`, `command_id`,
`runtime_operation_id`, and `automatic_resubmit=false`. This is an execution
fact, not a protocol error. Server-owned retry/failure policy decides what
happens next.

#### Scenario: Crash after dispatch does not double-submit

- **WHEN** the daemon crashes after the runtime call and cannot determine
  whether it was received, running, complete, or failed
- **THEN** restart records unknown execution, emits the explicit recoverable-
  false failure fact, and does not invoke the side-effecting operation again

#### Scenario: Reattachment resolves outcome

- **WHEN** runtime reattachment by stable operation identity proves completion
  or failure
- **THEN** the daemon records that terminal outcome without invoking a second
  operation

### Requirement: `message.send` has separate transport and logical identity

For `message.send`, `command_id` SHALL identify transport/delivery and
`message_id` SHALL identify the logical conversation message. The server SHALL
create one immutable command mapping for that logical message and retries SHALL
reuse the same command ID, message ID, and content. The daemon SHALL enforce
one `(session_id, message_id)` mapping to one command ID and immutable content.
A same-message/different-command delivery SHALL be rejected as an
integrity/protocol error with no runtime submission. One logical message SHALL
therefore result in at most one automatic runtime submission across reconnect
and daemon restart.

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

The daemon SHALL atomically allocate `daemon_event_seq` and append each event
before transmitting it. The server SHALL process an event at most once inside
the transaction that validates its identity/payload, commits its business
effect or durable rejection record, and records dedupe state. It SHALL send
`event_ack(status=accepted)` only after an effect commit and
`event_ack(status=rejected)` only after a durable rejection commit. Both are
terminal transport acknowledgements for that exact event identity/sequence. A
rejected ACK SHALL NOT request retry. Rollback, protocol error, or no ACK
leaves the original event replay-eligible.

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

The server SHALL send one finite connection-level `ReconcileSnapshot` after
authentication and before application readiness. It SHALL contain at most one
unique state entry per session pinned to that daemon and MAY contain zero
entries. For each session, `command_ack_through_seq` SHALL mean every command
at or below that sequence is durably known by the daemon, based only on
server-recorded durable `command_ack` results. `event_ack_through_seq` SHALL
mean every daemon event at or below it is durably handled by the server;
`event_ack_sparse` SHALL identify individually handled event sequences above
that watermark.

The server MAY compact outbox payloads at or below the confirmed command
watermark after retaining identity/tombstone state. Commands above it that are
not durably ACKed remain eligible for resend with original ID/sequence/payload
in ascending order. The daemon SHALL not replay event payloads at or below the
event watermark or listed in sparse ACKs; every other journaled event remains
replay-eligible and replay SHOULD be ascending by `daemon_event_seq`.

#### Scenario: Snapshot handles no pinned sessions

- **WHEN** an authenticated daemon owns no execution sessions
- **THEN** the server sends one valid snapshot with an empty `sessions` list

#### Scenario: Late command is inert

- **WHEN** a command at or below the daemon's durable processed/accepted
  watermark arrives with matching retained identity
- **THEN** the daemon never invokes runtime and returns the known
  `command_ack` where appropriate

#### Scenario: Sparse event ACK suppresses replay

- **WHEN** event sequence 7 appears in durable `event_ack_sparse` while the
  event watermark is 4
- **THEN** event 7's payload is not replayed, while unacknowledged event 5 or 6
  remains eligible

### Requirement: Sequence gaps are bounded and business-inert

For each session and direction, a valid frame above the next expected sequence
MAY be durably buffered but SHALL NOT affect business state until the gap
closes. North 0.1 SHALL enforce a finite configurable
`max_gap_buffer_entries_per_session` with default 256 across pending durable and
in-memory records. On capacity exhaustion, the receiver SHALL withhold ACK and
business application and close at a retryable reconciliation boundary; it
SHALL NOT require unlimited memory and SHALL NOT misclassify a valid gap as a
`protocol.error`.

Identity validation SHALL enforce:

```text
same sequence + same id + same payload  -> duplicate; inert / re-ack
same sequence + different id             -> protocol/integrity error
same id + different sequence             -> protocol/integrity error
same id + same sequence + different payload -> protocol/integrity error
```

#### Scenario: Out-of-order event cannot mutate state

- **WHEN** event sequence 4 arrives before sequence 3
- **THEN** sequence 4 is bounded-buffered or reconciliation is initiated, and no
  sequence-4 business effect occurs before sequence 3 is handled

#### Scenario: Conflicting sequence closes safely

- **WHEN** two frames use the same session sequence with different IDs
- **THEN** the receiver performs no business mutation, sends a directional
  `protocol.error`, and closes the current connection

### Requirement: Compaction retains durable duplicate protection

Compaction SHALL remove payload only after its delivery boundary is durable:
server command payloads after durable daemon ACK and contiguous command
watermark; daemon command payloads after terminal processing and contiguous
processed watermark; daemon event payloads after durable accepted or rejected
ACK. Compaction SHALL retain per-session watermarks and compact ID/payload
identity tombstones sufficient to make late duplicates inert and conflicts
visible. The daemon's `processed_through_seq` or equivalent SHALL NOT expire by
time alone while the durable session identity remains relevant. Server event
dedupe information SHALL never be deleted in a way that permits late replay to
reapply a business effect. Raw payload retention is not required once compact
identity, sequence, digest, and outcome protection is durable.

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

Hello and welcome SHALL carry exact `protocol_version: "0.1"`; 0.1.x frames
SHALL use `schema_version: 1`. `protocol.error` SHALL be a connection/control
frame in both `ServerFrame` and `DaemonFrame`. Server-to-daemon errors report
daemon protocol violations; daemon-to-server errors report server protocol
violations. Unknown frame/command/event types, unsupported schema, invalid
identity reuse, and incompatible versions SHALL cause no business side effect,
shall receive a protocol error where a valid error frame can be encoded, and
shall close the current connection. A protocol error SHALL have no severity
discriminator. Receiving one is terminal to the current connection; future
reconnect is only a host policy. Unacknowledged durable work remains eligible
for replay.

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

North 0.1 SHALL use JSON text WebSocket messages with an Axum server adapter
and a `tokio-tungstenite` daemon supervisor. The daemon supervisor SHALL own
hello, split reader/writer tasks, ping/pong, bounded queues, disconnect,
transport backoff, and reconnect. It SHALL expose `Welcome` and the complete
`ReconcileSnapshot` to coordination and SHALL wait for coordination readiness
before `Active`, heartbeat, events, ACKs, replay, or commands. Transport
liveness SHALL NOT imply durable delivery or business completion.

#### Scenario: Snapshot receipt does not activate traffic

- **WHEN** a valid snapshot arrives but coordination has not applied it and
  signaled ready
- **THEN** application traffic remains gated and the daemon is not `Active`

### Requirement: Context and readiness authority remain server-owned

`session.start` SHALL contain complete server-assembled Requirement context,
bounded relevant conversation, and enabled repository metadata without
credentials, checkout paths, persistence handles, or domain types.
`requirement.assessed` SHALL carry typed evidence; the server SHALL continue to
apply revision-bound evidence, `state_version` concurrency, assessment identity,
and accepted-generation rules in its own transaction. The daemon reports facts
and never directly mutates Requirement state. `session.resume` SHALL contain
execution recovery only and no transport event cursor.

#### Scenario: Delivered assessment cannot bypass server gates

- **WHEN** a durable assessment event targets a stale revision or state version
- **THEN** server coordination rejects or handles it using existing domain and
  transaction rules, regardless of successful transport delivery

#### Scenario: Resume does not own replay state

- **WHEN** a `session.resume` command is encoded
- **THEN** it contains execution-recovery data only; directional event sequence
  and replay watermarks remain in reconciliation state

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
