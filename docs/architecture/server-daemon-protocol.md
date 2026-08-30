# Server ↔ daemon protocol

Canonical message catalog: `crates/north-protocol/src/lib.rs` + the canonical
`openspec/specs/daemon-protocol/spec.md` contract. Daemon connection and distributed
architecture guardrails are established prerequisites in the canonical OpenSpec
specs. This doc fixes the wire and reconciliation contract; it does not make
the server or daemon own each other's business logic.

## Transport boundary

North 0.1 standardizes server↔daemon communication as:

```text
server: Axum WebSocket
wire:   North protocol JSON text frames
daemon: tokio-tungstenite
```

Axum and tokio-tungstenite own HTTP/WebSocket upgrade, RFC 6455 framing, text
transport, ping/pong, close frames, socket lifecycle, TLS integration, and
transport errors. North does not implement WebSocket framing or a Socket.IO-like
layer. Each North frame serializes to one WebSocket **text** message; binary
messages are rejected at the adapter boundary.

Current defensive defaults are 8 MiB maximum message size, 1 MiB maximum frame
size, a bounded 64-item admission queue, and bounded 256-item inbound/outbound
queues. The daemon
uses tokio-tungstenite with the minimal `rustls-tls-native-roots` feature for
WSS. These are configuration points, not reliability guarantees.

## Transport & topology

- Daemon-initiated persistent connection (WebSocket over TLS in deployment);
  the daemon may sit behind NAT/firewalls; no inbound ports.
- The server endpoint is an Axum upgrade handler plus a thin transport adapter;
  the adapter starts the hello deadline immediately after upgrade, reads the
  first `hello`, and admits the hello-bearing connection through a bounded
  coordinator queue with its own timeout. Server transport config owns only
  hello and admission bounds; daemon-side config owns welcome, reconciliation,
  and coordination-readiness deadlines. Coordination authenticates the hello,
  owns registration, and receives decoded frames through bounded channels.
- The daemon endpoint is one `tokio-tungstenite` connection supervisor; it
  owns hello, split reader/writer tasks, ping/pong, disconnect, local backoff,
  and reconnect. Runtime/session code never writes to the socket directly.
- Daemon authenticates with a locally stored CLI/daemon credential obtained via
  browser setup — never a reused email verification code. Browser approval GET
  returns a read-only HTML confirmation page (or JSON for explicit API clients);
  authenticated same-origin POST is the only approval mutation. The polling CLI
  alone uses the claim endpoint that returns the one-shot daemon credential, and
  approval HTML never contains it.
- Hello/registration and welcome carry exact `protocol_version: "0.1"`;
  each frame carries `schema_version: 1`. 0.1.x has no plugin/range
  negotiation.
- After auth the daemon registers one durable identity plus capabilities;
  heartbeats maintain liveness. `created_by` is the account that created the
  user-owned credential. Admin/Owner can revoke it; revocation closes current
  access and refuses future handshakes.

## Frame groups — direction is part of a message's identity

A message belongs to exactly one group. Nothing appears in both directions
under the same name.

```text
Connection/control frames
  daemon → server : hello/registration, heartbeat, command_ack, protocol.error
  server → daemon : welcome, event_ack(status=accepted), event_ack(status=rejected),
                    reconciliation snapshot, protocol.error

Server commands (server → daemon ONLY)
  session.start · session.cancel · session.resume · message.send

Daemon events (daemon → server ONLY)
  session.started · agent.message · agent.activity · requirement.assessed
  session.completed · session.failed

`protocol.error` is bidirectional connection/control traffic. The sender reports
that its peer violated the North protocol: daemon → server reports a server
violation, and server → daemon reports a daemon violation. It has no severity
field, and sending or receiving it closes only the current connection.
```

`command_ack` means the daemon durably recorded the command for processing;
it is not runtime completion and carries no status. `event_ack(status=accepted)` means
the server committed the business effect. `event_ack(status=rejected)` means the server durably recorded a
well-formed fact as rejected (for example stale assessment) and will not retry
its business effect. No success ACK is sent before its relevant commit.

`session.resume` is a server COMMAND for execution recovery only; it carries
no daemon-event cursor. Transport replay state belongs exclusively to
`ReconcileSnapshot` ACK watermarks and canonical `event_ack_sparse` lists. It is
never a daemon→server event.

## Reconciliation and activation

`reconcile` is one finite connection-level `ReconcileSnapshot`, not one frame per
session. Its `sessions` list may be empty for a daemon with no pinned sessions,
or contain one unique `SessionReconcileState` per pinned session. Each entry
contains independent command/event contiguous watermarks and a canonical
`event_ack_sparse` list. The list is strictly ascending, contains no duplicates,
and has only values above `event_ack_through_seq`. The protocol validates non-empty
session IDs, canonical sparse ACKs, and duplicate-session rejection before
coordination sees the snapshot.

The daemon supervisor delivers `Welcome` plus `ReconcileSnapshot` as one
`HandshakeResult` to coordination, then waits for coordination to apply/restore
replay state and signal readiness under one total coordination-stage timeout.
Only that signal moves the connection from
`ReconciliationReceived` to `Active`; ping/pong may operate before then, but
heartbeat, events, and ACKs cannot race ahead; any future replay traffic will
use the same gate.

## `session.start` runtime context

The server assembles complete runtime context before dispatching `session.start`.
The North wire DTO contains:

- `requirement`: id, revision, title, description, summary,
  acceptance criteria, assumptions, and open questions;
- `conversation`: bounded/relevant message excerpt only, never full durable
  history by default;
- `repositories`: enabled repository metadata (`repository_id`, name, URL,
  description) only.

Repository credentials, tokens, SSH keys, local checkout paths, persistence
handles, and `north-domain` types never cross this boundary. Server conversion
builds these DTOs and filters disabled repositories.

## Envelope and delivery contract

The durable-delivery rules below define the North 0.1 contract. The wire,
transport, server outbox/event ledger, daemon local journal, replay, bounded gap
handling, reconciliation, and identity tombstones are implemented. The daemon
journal retains full payloads until explicit safe compaction; business execution
retry and the real agent runtime remain downstream responsibilities.
`requirement.assessed` evidence, repository citation gates, revision checks, and
post-commit ACKs are implemented by the server readiness path.

Current generic-event handling is deliberately only protocol delivery: the
server validates event identity and sequence, records one durable accepted or
rejected receipt, sends the matching terminal ACK after commit, and suppresses
replay after that ACK. Only `requirement.assessed` has a business projection in
this slice. `session.started`, `agent.message`, `agent.activity`,
`session.completed`, and `session.failed` are durably rejected with
`event_handler_not_implemented`; they do not transition execution state,
project activity/conversation data, consume retry budget, or claim that server
retry policy ran. Those projections belong to later runtime/retry changes.

Only delivery envelopes carry envelope fields:

- Every `CommandEnvelope` carries `command_id`, `session_id`,
  `server_command_seq`, `sent_at`, `schema_version`, and a typed
  command.
- Every `EventEnvelope` carries `event_id`, `session_id`,
  `daemon_event_seq`, `sent_at`, `schema_version`, and a typed event.
- Connection/control frames use their own schemas and do not require
  `session_id`. `hello`, `welcome`, `heartbeat`, the finite
  `ReconcileSnapshot`, and `protocol.error` are connection-level frames.
  `command_ack` and `event_ack` copy the target identity, session, and
  sequence, but are acknowledgements rather than envelopes.

The server persists an execution-session owner and complete immutable command
outbox row atomically before dispatch through
`AuthStore::start_session_with_command`. The row includes the original envelope
payload and digest. `DaemonRuntime::persist_and_dispatch_command` dispatches
only that persisted representation; authenticated daemon `command_ack` updates
the durable contiguous watermark, and reconnect sends unacknowledged rows in
per-session sequence order. The daemon-side `Journal`/`DaemonCoordinator`
implements the local command inbox, processed tombstones, event journal, replay,
and duplicate suppression.

The durable-delivery contract requires daemon events to be journaled before
transmission. Command and event sequence allocation commits atomically with the
corresponding outbox/journal record; a failed transaction cannot create a
committed sequence hole. The server command outbox and daemon event journal
retain the original stable ID, sequence, serialized payload, and payload digest
for replay.

Daemon command records move monotonically through `received`,
`dispatch_started`, and `terminal`. `received` is durable receipt and may
produce `command_ack`; `dispatch_started` is committed before a runtime
operation; terminal records the local outcome of processing/dispatching that
specific command (`dispatch_succeeded`, `dispatch_failed`, or `unknown`; local
`completed`/`failed` labels are allowed). This terminal command outcome does not
mean the execution session is terminal: `session.start` may dispatch
successfully while the session remains `Running`, followed later by a separate
`session.completed` or `session.failed` event.

The durable command coordinator decides journal state and idempotency before
crossing a narrow internal dispatch/execution seam that accepts stable command
and runtime-operation identity. Durable-delivery tests may use a deterministic
fake executor. The shipped `north-daemon` binary currently wires a `LocalRuntime` placeholder.
It performs durable protocol/runtime coordination but has no production agent
runtime adapter; executable commands therefore surface a not-configured/unknown
execution fact. The future `introduce-agent-requirement-clarification` change
provides the real adapter. This protocol change does not introduce agent
prompting, SDK behavior, tool choice, repository inspection, or readiness
judgment.

A crash between dispatch and outcome first attempts reattachment by
`runtime_operation_id = command_id`. If outcome remains unknowable, the
existing runtime operation cannot be safely recovered locally, so the daemon
emits journaled `session.failed` with `recoverable: false`,
`execution_outcome_unknown`, the command/runtime identity, and
`automatic_resubmit=false`. `recoverable` is only the daemon's fact about local
resume/reattach ability; `false` means the existing operation cannot be safely
recovered locally. Either value leaves server retry/failure policy authoritative.
The server alone owns execution-attempt count, retry budget, `session.resume`
policy, and final execution `Failed` state. The daemon never blindly resubmits
a side-effecting operation;
any later attempt is an explicit server-directed command with a new identity.

The readiness path validates assessment event identity, session binding,
and `daemon_event_seq`/sequence identity before comparing
`requirement_revision` with the current Requirement revision. It then runs
server/domain readiness gates, atomically records immutable evidence and any
valid `Discussing` -> `Ready` promotion, records the resulting Ready-generation
`state_version` as `accepted_state_version`, commits, and sends
`event_ack(status=accepted)` (or commits a rejection and sends
`event_ack(status=rejected)`). Generic runtime events use the same durable
identity/sequence/rejection boundary but have no business projection here:
`session.started`, `agent.message`, `agent.activity`, `session.completed`, and
`session.failed` receive `event_handler_not_implemented`, a durable rejected
receipt, and `event_ack(status=rejected)`. No execution-state transition,
retry-budget decision, or activity/conversation projection is implied.
Accepted evidence creates/binds `assessment_id`
and `accepted_state_version`; neither is an inbound assessment concurrency
token. Later human Accept, Reject, or Request Changes uses `assessment_id`,
`expected_state_version`, and the exact current Ready generation. The daemon
never writes Requirement state directly. Repository citation existence and
session/run provenance are checked by server readiness persistence; the wire
layer also rejects malformed or incomplete Git SHA-1/SHA-256 `commit_sha` values.

## Sequence and reconnect rules

`command_id` and `event_id` are globally unique opaque identities in their
respective namespaces. `server_command_seq` and `daemon_event_seq` are
independent monotonic counters scoped to one session and direction. They start
at 1 and commit atomically with their outbox/journal record. The durable stores
must enforce unique ID and `(session_id, sequence)` mappings; generation alone
is not a uniqueness guarantee.

Each `SessionReconcileState` carries `command_ack_through_seq`,
`event_ack_through_seq`, and a strictly ascending, unique sparse event
sequence list above the event watermark when handling is non-contiguous.
`command_ack_through_seq` means every command at or below it is durably known
by the daemon, based only on server-recorded durable command ACKs.
`event_ack_through_seq` means every daemon event at or below it is durably
handled by the server; sparse entries are individually handled events.

- A duplicate with same ID, sequence, and payload is inert and receives the
  known ACK again.
- Same sequence with a different ID, same ID with a different sequence, or same
  ID/sequence with a different payload is a protocol/integrity error with no
  business effect.
- A valid out-of-order frame may be durably buffered, but cannot affect business
  state until the gap closes. Pending durable/in-memory records are bounded by
  the configured `max_gap_buffer_entries_per_session` (default 256). Overflow
  withholds ACK and closes the connection at a retryable reconciliation boundary;
  it is not a protocol error.
- The server resends unacknowledged outbox commands in ascending
  `server_command_seq` with their original ID/sequence/payload.
- The daemon replays unacknowledged event journal records in ascending
  `daemon_event_seq`, after applying the snapshot's event watermark and sparse
  ACKs. ACKed event payloads are not replayed.

IDs answer “is this the same delivery?” Sequences answer “is this the next
ordered delivery?” Neither replaces the other.

Payloads may be compacted only after the corresponding durable delivery
boundary. Server command, daemon command, daemon event, and server event-dedupe
stores retain high-water state plus compact ID/digest tombstones so late
duplicates remain inert and cannot reapply business effects. The daemon's
`processed_through_seq` is not expired by time alone while the session remains
relevant. Requirement Accepted/Rejected never by itself authorizes transport
compaction; execution-session delivery state does.

## Compatibility and errors

A protocol-version mismatch receives `protocol.error(incompatible_protocol)`
from the receiver's direction-specific frame enum and the connection closes
before session traffic. Unknown command/event types, unsupported schema
versions, and identity/payload conflicts receive explicit `protocol.error`,
cause no side effect, and close the connection. `protocol.error` is present in
both `ServerFrame` and `DaemonFrame`; it carries no severity discriminator and
is terminal to the current connection. The host decides whether an equivalent
future connection may be attempted. Unacknowledged outbox/journal messages
remain eligible for replay; peers never silently reinterpret unknown payloads.

## Liveness and error boundaries

WebSocket ping/pong means the socket can exchange transport control frames. The
North `heartbeat` frame means an authenticated daemon is alive and reporting
application state. Daemon status expires Live after 45 seconds without a
heartbeat, even if the socket has not reported close. Ping/pong never proves
durable session state, command processing, or liveness after reconnect.

Transport failures stay distinct from protocol failures:

- transport: connection reset, TLS failure, invalid WebSocket frame, or close;
- protocol: unsupported version, malformed North JSON, unknown frame/command,
  schema failure, sequence violation, authentication, or reconciliation failure.

Adapters return transport errors and decode errors to the protocol/session
coordinator. That layer may send `protocol.error` and close the peer where the
contract requires it; no decoder error mutates business state.

## Reliability ownership

Axum and tokio-tungstenite do not provide durable messaging. North coordination
owns stable command/event IDs, monotonic sequences, at-least-once delivery, the
server command outbox, daemon processed-command dedupe, daemon event journaling,
ACK-after-commit, bounded gap handling, reconnect reconciliation, session
ownership, retry policy, and Requirement transaction semantics. The current
implementation provides the wire/transport boundaries, daemon registration and
revocation, server ACK/event ledgers, and the daemon Journal coordinator. Business
execution retry and the real agent runtime remain downstream responsibilities.

## Session routing and state ownership

The current session-routing flow selects a connected eligible daemon and persists
its identity before the first command. `DaemonRuntime::persist_and_dispatch_command`
constructs and dispatches the persisted envelope only through that owner, while
inbound events and ACKs from a different daemon receive a protocol error.
Reconnect receives one reconciliation snapshot for the same identity, resends
unacknowledged commands, and leaves the session pinned; revocation closes only
that connection and leaves durable work eligible for a future authorized
connection. Business retry/failure policy remains server-owned and downstream
of this delivery layer.

The implemented readiness flow is: Agent produces a readiness assessment →
daemon emits `requirement.assessed` → server deduplicates and locks the
session-bound Requirement, validates event revision and domain gates, persists
typed verdict, blockers, assumptions, and reviewed repository SHAs plus any
valid transition, commits, then sends `event_ack(status=accepted)` (or commits
a rejection and sends `event_ack(status=rejected)`). The daemon never writes
Requirement state directly.
