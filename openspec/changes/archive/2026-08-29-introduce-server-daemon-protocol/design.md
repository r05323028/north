# Design

## Context

Wire format must survive reconnects, retries, and future evolution without
leaking business types across crates. Axum and `tokio-tungstenite` provide
transport only. North coordination owns durable delivery, idempotency,
ordering, replay, acknowledgement, recovery, session ownership, and server
business policy.

The daemon connection, identity, readiness gate, session pinning, and typed
wire foundation already exist. This design fills the remaining durable
contract without changing that topology or claiming exactly-once execution
from an underlying runtime that cannot provide it.

## Decisions

### Transport and frame direction

North 0.1 uses one JSON text value per WebSocket text message. Binary messages,
malformed JSON, unknown frame types, unsupported schema versions, and configured
size violations have no business side effect. The protocol crate exposes no
Axum, Tokio, Tungstenite, or WebSocket types.

Frame direction is explicit:

```text
connection/control
  daemon -> server: hello, heartbeat, command_ack, protocol.error
  server -> daemon: welcome, event_ack, reconciliation, protocol.error

server commands (server -> daemon only)
  session.start, session.cancel, session.resume, message.send

daemon events (daemon -> server only)
  session.started, agent.message, agent.activity, requirement.assessed,
  session.completed, session.failed
```

`protocol.error` is present in both `ServerFrame` and `DaemonFrame` with the
same payload schema. It reports a peer violation in the direction that sends
it. It has `schema_version`, `code`, and `message`, no severity discriminator,
and no required `session_id`. Sending or receiving one closes the current
connection and stops that supervisor run; a host may make a future connection
attempt according to its existing terminal-failure policy. A protocol error
must never be answered with another protocol-error loop.

The existing handshake remains server `Welcome`, then one server-to-daemon
connection-level `ReconcileSnapshot`. The snapshot may contain zero or more
pinned sessions. The daemon passes both values to coordination and reaches
`Active` only after coordination applies reconciliation and signals readiness.

### Envelope and identity contract

Only command and event envelopes use delivery-envelope fields:

```text
CommandEnvelope
  command_id
  session_id
  server_command_seq
  sent_at
  schema_version
  typed command

EventEnvelope
  event_id
  session_id
  daemon_event_seq
  sent_at
  schema_version
  typed event
```

`command_id` is a globally unique opaque identity in the command namespace.
`event_id` is a globally unique opaque identity in the event namespace. UUID,
ULID, or another collision-resistant opaque value is acceptable, but the
persistence layer MUST enforce uniqueness rather than trusting generation.

`server_command_seq` is a unique monotonic order position within one
`session_id` in the server-to-daemon direction. `daemon_event_seq` is a unique
monotonic order position within one `session_id` in the daemon-to-server
direction. Each starts at 1. Neither sequence is global and neither replaces
its direction's stable ID.

Connection/control frames have separate schemas:

- `hello`: protocol version, schema version, daemon identity, credential, and
  capabilities;
- `welcome`: protocol version, schema version, daemon identity, and server time;
- `heartbeat`: schema version, daemon identity, timestamp, and application
  state;
- `ReconcileSnapshot`: schema version plus per-session reconciliation entries;
- `protocol.error`: schema version, code, and message.

These connection-level frames are not envelopes and do not require
`session_id`. `command_ack` and `event_ack` are delivery-control frames: they
copy the exact target ID, `session_id`, and sequence they acknowledge, but are
not command/event envelopes.

The durable uniqueness contract is:

```text
server outbox:       UNIQUE command_id
                     UNIQUE (session_id, server_command_seq)
daemon command log:  UNIQUE command_id
                     UNIQUE (session_id, server_command_seq)
daemon event log:    UNIQUE event_id
                     UNIQUE (session_id, daemon_event_seq)
server event log:    UNIQUE event_id
                     UNIQUE (session_id, daemon_event_seq)
```

A repeated identity is valid only when session, sequence, type, and payload
(or its durable payload digest) match the original. Any mismatch is an
integrity/protocol error, has no business effect, and closes the connection.

### Atomic sequence allocation

For a server command, one durable transaction MUST:

1. lock the session's durable command-sequence allocator;
2. select the next sequence value;
3. create the globally unique `command_id` and outbox record containing the
   complete immutable envelope; and
4. commit the allocator update and outbox row together.

The allocator may be a row protected by a transaction lock or an equivalent
transactional mechanism. A best-effort process counter or a non-transactional
sequence whose rolled-back values become holes is not sufficient. A failed
transaction consumes no sequence value; the same next value can be allocated
later. Retries reuse the committed ID, sequence, and serialized payload.

For a daemon event, one durable local-journal commit MUST allocate the next
`daemon_event_seq` and append the complete event record together. The journal
must flush/commit both the allocation and record as one durable unit. A crash
before that commit leaves no event in the durable sequence space and permits
reuse of the same next sequence value.

### Server command outbox

The server inserts a complete command before dispatch. Each command has one
immutable `command_id`, sequence, session, command type, payload, and payload
digest. Dispatch/reconnect retries send the exact stored envelope in ascending
`server_command_seq` order.

A daemon `command_ack` is durably incorporated by the server before the server
considers that command durably received. ACK processing verifies command ID,
session, sequence, and stored digest. A valid duplicate ACK is inert. A
conflicting ACK is a protocol/integrity error. Server coordination maintains a
contiguous `command_ack_through_seq` per session; it advances only across
actually acknowledged commands, never across liveness or socket state.

### Command dispatch/execution seam

The durable command coordinator is the owner of journal state and idempotency.
It MUST decide whether a command may cross the execution boundary before
crossing it. The narrow internal seam accepts the stable command identity and
stable `runtime_operation_id` (equal to `command_id` in 0.1.0), plus the
already-validated runtime operation input. Its exact Rust type/name is not a
wire contract. Durable-delivery tests MAY provide a deterministic fake/test
executor. Duplicate or replayed commands cross the seam only when the command
journal state machine permits it; terminal or otherwise duplicate-safe records
never cross it again. The later `introduce-agent-requirement-clarification`
change supplies the real agent-runtime adapter behind this seam. This protocol
change adds no prompting, SDK behavior, tool choice, repository inspection, or
readiness judgment.

### Daemon command journal state machine

Each command has one monotonic local delivery state:

```text
received
  complete command record and payload digest durably appended;
  command_ack MAY be sent after this commit; runtime has not been invoked.

dispatch_started
  durably recorded before invoking a runtime operation whose duplicate could
  matter; runtime_operation_id is stable and equals command_id in 0.1.0.

terminal
  no further automatic invocation is permitted; the local outcome of processing
  or dispatching this specific command is durable: dispatch_succeeded,
  dispatch_failed, or unknown. An implementation MAY use completed/failed
  labels for the first two local outcomes; neither means the execution session is
  terminal.
```

`received -> dispatch_started -> terminal` is the only normal progression.
A duplicate delivery in any state returns the known `command_ack` when needed
and never invokes the runtime twice. A crash in `received` may continue
processing after restart. A crash after `dispatch_started` cannot restart by
blind invocation; it follows crash recovery below.

`command_ack` means only that the daemon durably recorded the command for
processing. It does not mean the runtime completed it.

### Crash-after-dispatch and unknown outcome

The daemon MUST persist `dispatch_started` before calling the runtime. If the
process crashes after that call and before terminal outcome commit, restart
MUST first attempt runtime reattachment/status recovery using
`runtime_operation_id = command_id` where the runtime supports it.

If recovery proves completion or failure, the daemon records that terminal
outcome without invoking the operation again. If recovery cannot determine
whether the runtime never received the operation, is still running, completed,
or failed, the daemon records terminal `unknown` and emits:

```text
session.failed {
  recoverable: false,
  reason: "execution_outcome_unknown command_id=<command_id> runtime_operation_id=<command_id> automatic_resubmit=false"
}
```

`recoverable` is a daemon fact about the existing runtime operation: `true`
means the daemon believes that operation can be safely resumed or reattached
by local mechanics; `false` means it cannot safely do so. It does not mean the
server is forbidden from issuing a future explicit attempt, and it does not
make the daemon the authority for execution failure. In both cases the server
chooses whether to issue a new `session.resume`/`session.start` attempt or mark
execution `Failed`. For `execution_outcome_unknown`, the daemon MUST NOT
resubmit automatically; any later attempt is explicitly server-directed and
uses a new command identity under server retry policy.

The reason is an execution fact and contains stable operation identity, not
secret payload. This event is journaled and ACKed like every other event. It
is not a `protocol.error`, does not mutate Requirement state, and does not
assert exactly-once runtime execution.

### `message.send` identity

`command_id` identifies transport/delivery; `message_id` identifies the
logical conversation message. The server creates the logical message and its
one command mapping durably, and retries reuse the same command ID, message ID,
and content.

The daemon command journal enforces one `(session_id, message_id)` mapping to
one command ID and immutable content. The same command ID with different
message ID/content is an integrity error. The same message ID under a different
command ID is also an integrity error and causes no runtime submission; the
server is required never to create that mapping. Thus one `message.send`
command identity and one logical message identity produce at most one automatic
runtime submission, including reconnect and daemon restart replays.

### Event journal and ACK semantics

The daemon allocates `daemon_event_seq` and appends the complete event before
transmission. A replay uses the original event ID, sequence, timestamp, type,
and payload. The daemon does not allocate a new event identity for resend.

The server handles an event in one transaction that validates its identity and
payload digest, records event dedupe/rejection state, writes immutable evidence
where applicable, and applies any business transition. For
`requirement.assessed`, that transaction validates event identity, session
binding, `daemon_event_seq`/sequence identity, and `requirement_revision`
against the current Requirement revision before running server/domain readiness
gates. It then atomically records immutable evidence and any valid
`Discussing` -> `Ready` promotion, recording the resulting Ready-generation
`state_version` as `accepted_state_version`. Accepted evidence creates/binds its
`assessment_id` and `accepted_state_version`; neither is an inbound assessment
concurrency token. Later human review uses `assessment_id`,
`expected_state_version`, and the exact current Ready generation. The wire
`north-protocol` layer checks only structurally non-empty `repository_id` and
`commit_sha`; repository existence and session/run provenance belong to server
readiness persistence. The server remains authoritative for readiness.

After commit:

- `event_ack(status=accepted)` means the business effect committed;
- `event_ack(status=rejected)` means a well-formed fact and durable rejection
  record committed, such as stale revision evidence;
- both ACKs are terminal transport acknowledgements for that exact event ID and
  sequence;
- a rejected ACK never requests daemon retry;
- after either ACK is represented in durable reconciliation state, the daemon
  removes or compacts that event payload and does not replay it;
- rollback, protocol error, or no ACK leaves the original event replay-eligible.

A duplicate event with matching identity/payload returns the previously known
terminal ACK without applying business state again. A conflict is a protocol
error with no ACK for the conflicting frame.

### Reconciliation merge algorithm

`ReconcileSnapshot` remains one server-to-daemon connection-level snapshot sent
after authentication and before readiness. Each pinned session has one
`SessionReconcileState` with:

```text
command_ack_through_seq
  every command at or below this sequence is durably known by the daemon;
  the server may compact their full outbox payloads after retaining required
  identity/tombstone state.

event_ack_through_seq
  every daemon event at or below this sequence is durably handled by the server.

event_ack_sparse
  individually handled event sequences above event_ack_through_seq.
```

The server derives the command watermark only from durable daemon
`command_ack` processing. It does not infer it from connection liveness. The
server retains unacknowledged outbox records above the confirmed watermark and
resends them in ascending sequence order with original ID, sequence, and
payload. An ACKed record above the contiguous watermark is not eligible for
resend, but remains until the contiguous boundary makes compaction safe.

Before `Active`, daemon coordination merges the snapshot into its durable
session state. For event delivery, any journal payload with sequence at or
below `event_ack_through_seq` or present in `event_ack_sparse` is durably
handled and is not replayed. Every other event remains replay-eligible and is
replayed in ascending `daemon_event_seq`, subject to gap handling. A daemon
late duplicate command at or below its durable `processed_through_seq` looks
up its retained identity/tombstone, never invokes the runtime, and returns the
known `command_ack` where appropriate.

The snapshot is finite and contains one unique entry per pinned session. It is
not a per-session stream and it does not transfer business state ownership from
server to daemon.

### Gap and identity conflict handling

Each direction tracks its next expected sequence per session. A valid frame
above that sequence may be durably buffered, but it cannot affect business
state until the gap closes. North 0.1 uses a finite configurable
`max_gap_buffer_entries_per_session` (default 256) covering pending durable and
in-memory records. The implementation MUST NOT use unlimited in-memory
buffering.

When the bound would be exceeded, the receiver does not ACK or apply the new
frame and closes the connection at a retryable reconciliation boundary. The
frame remains eligible from its sender's outbox/journal; reconnect/replay must
fill the gap. This is not a `protocol.error` because the frame itself may be
valid. A persisted pending record may be used instead of memory, but the same
finite bound applies.

For both command and event directions:

```text
same sequence + same id + same payload
  duplicate; inert and re-acknowledge known result

same sequence + different id
  protocol/integrity error; no business effect; close connection

same id + different sequence
  protocol/integrity error; no business effect; close connection

same id + same sequence + different payload
  protocol/integrity error; no business effect; close connection
```

A late frame at or below a durable acknowledged/processed boundary is inert
when its retained identity and digest match. Conflicting identity data is
never allowed to alter a tombstone.

### Compaction and retention

Compaction removes payload, not the state required to reject late duplicates:

- **Server command outbox:** after daemon ACK is durable, delivery payload may
  be compacted only at or below the durable contiguous command ACK watermark.
  Retain per-session watermark plus compact command ID/payload-digest
  tombstones needed to validate late ACKs and sequence conflicts.
- **Daemon command journal:** full command records may be compacted after
  terminal processing and contiguous processed/accepted watermark. Retain
  durable `processed_through_seq` and compact per-sequence identity/digest
  tombstones while the session identity remains relevant. This tombstone is not
  expired by time alone in 0.1.0.
- **Daemon event journal:** full event payload may be removed after either
  accepted or rejected event ACK is durably incorporated into reconciliation.
  Retain event ACK watermarks/sparse state and compact event identity/digest
  tombstones sufficient to keep late replay inert.
- **Server event dedupe:** never delete all durable event identity information
  while a late replay could reapply a business effect. Retain an event
  identity/sequence/digest/outcome row or equivalent watermark/tombstone. Raw
  payload retention is not required once this compact protection is durable.

Session retirement, archival, and any deletion of these tombstones require an
explicit future lifecycle decision proving that no reconnect or late replay
can occur. A TTL alone is not safe for a still-relevant durable session.

### Terminal session boundary

Execution-session terminal state (`completed`, `failed`, or `cancelled`) is
transport-delivery state, not Requirement lifecycle state. Requirement
`Accepted`/`Rejected` must never by itself authorize outbox/journal compaction.
A terminal execution session can retain unacknowledged commands/events until
its delivery watermarks and tombstones are safe. A Requirement can be Accepted
while its execution session still has delivery work. A command journal reaching
its local terminal dispatch outcome does not end the execution session: for
example, `session.start` dispatch may succeed while the session remains
`Running`, followed later by a separate `session.completed` or `session.failed`
event.

### Context, ownership, and retry boundaries

The server assembles complete `session.start` context: current structured
Requirement content/revision, bounded relevant conversation, and enabled
repository metadata. No credentials, checkout paths, persistence handles, or
`north-domain` values cross `north-protocol`.

The daemon owns socket reconnect/backoff and local journal recovery only. The
server owns business retry/failure policy, session ownership, Requirement
transitions, readiness evidence, and all `revision`/`state_version`/
`assessment_id`/`accepted_state_version` transaction rules. Protocol delivery
of a fact never grants the daemon authority to mutate Requirement state.

The current server event seam is intentionally narrow. It validates identity,
sequence, and payload integrity, records a durable accepted or rejected event
receipt, and emits the matching ACK only after commit. `requirement.assessed`
has the only business projection in this change. `session.started`,
`agent.message`, `agent.activity`, `session.completed`, and `session.failed`
are durably rejected with `event_handler_not_implemented`; execution-state,
activity/conversation, and retry-budget projections remain deferred to their
own changes. A rejected `session.failed` fact is still retained and replay-safe;
it does not mean server retry policy was applied.

The binary's `LocalRuntime` is an explicit placeholder: durable coordination is
wired, but no production agent runtime adapter exists yet. Its
`runtime_adapter_not_configured` unknown outcome is an execution fact, not a
claim of final server execution failure. The real adapter remains owned by
`introduce-agent-requirement-clarification`.

## Risks / Trade-offs

- **Crash after a side-effecting runtime call** → stable operation identity and
  reattachment are attempted; otherwise explicit unknown outcome is reported,
  with no automatic resubmission.
- **Journal growth** → compact payloads at durable watermarks while retaining
  compact identity/tombstone protection; never use time-only expiration for an
  active session.
- **Gap pressure** → bounded pending storage closes the current connection and
  relies on durable sender replay instead of unbounded memory.
- **Strict protocol evolution** → incompatible peers fail closed with a
  terminal bidirectional `protocol.error`; no severity or plugin negotiation.
- **At-least-once delivery** → duplicate frames are expected and made inert;
  exactly-once runtime execution is not claimed.
