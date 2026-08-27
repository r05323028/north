# Server ↔ daemon protocol

Canonical message catalog: `crates/north-protocol/src/lib.rs` + OpenSpec change
`introduce-server-daemon-protocol`, hardened by
`harden-distributed-system-architecture`. This doc fixes the wire and
reconciliation contract; it does not make the server or daemon own each
other's business logic.

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
  browser setup — never a reused email verification code.
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
  daemon → server : hello/registration, heartbeat, command_ack
  server → daemon : welcome, event_ack(status=accepted), event_ack(status=rejected),
                    reconciliation snapshot, protocol.error

Server commands (server → daemon ONLY)
  session.start · session.cancel · session.resume · message.send

Daemon events (daemon → server ONLY)
  session.started · agent.message · agent.activity · requirement.assessed
  session.completed · session.failed
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

The durable-delivery rules below define the North 0.1 target contract. The wire
representation and transport boundaries exist today, but the durable server
command outbox, daemon command/event journals, ACK-after-commit persistence,
replay, gap reconciliation, and high-watermark compaction remain pending
implementation. Statements in this section use normative language for that target
unless they describe current transport behavior explicitly.

Every command/event carries:

- stable `command_id` or `event_id` for idempotency;
- `session_id` and a direction-specific sequence (`server_command_seq` or
  `daemon_event_seq`) for ordering and gap detection;
- `sent_at`, `type`, `payload`, and `schema_version`.

The current session-routing foundation persists an execution-session owner and
server command outbox row atomically before dispatch through
`AuthStore::start_session_with_command`. The server assigns sequence metadata,
serializes one complete envelope, and `DaemonRuntime::persist_and_dispatch_command`
dispatches the persisted payload; full command ACK semantics, retries, local
command inbox/processed-ledger durability, and duplicate runtime suppression
remain owned by the durable-delivery implementation.

The durable-delivery contract requires daemon events to be journaled before
transmission. The server will deduplicate event ids inside the same transaction
as validation/evidence/business state. Successful ACKs will follow commit;
unacknowledged daemon events will remain buffered and will be replayed after
reconnect.

## Sequence and reconnect rules

`server_command_seq` and `daemon_event_seq` are independent monotonic counters,
scoped to one session and direction. They start at 1; the durable-delivery layer
will persist each assigned value with the relevant outbox/journal record. Each
`SessionReconcileState` in the connection-level snapshot carries
`command_ack_through_seq`, `event_ack_through_seq`, and a strictly ascending,
unique sparse event sequence list above the event watermark when processing is
non-contiguous.

- A duplicate id+sequence is harmless and receives the known ACK again.
- One sequence with a different id is a protocol error.
- The durable-delivery contract permits an out-of-order frame to be durably
  buffered, but it will not be applied until the missing sequence is
  replayed/reconciled.
- A late frame at or below an acknowledged contiguous sequence is inert.
- The durable-delivery layer will replay buffered daemon events in ascending
  `daemon_event_seq`; server command retries will retain ascending
  `server_command_seq`.
- The durable-delivery contract permits processed command rows to be compacted
  only after terminal session reconciliation proves a contiguous watermark. A
  durable per-session `processed_through_seq` tombstone will remain, so old
  duplicates stay inert.

IDs answer “is this the same delivery?” Sequences answer “is this the next
ordered delivery?” Neither replaces the other.

## Compatibility and errors

A protocol-version mismatch receives `protocol.error(incompatible_protocol)`
and the connection closes before session traffic. Unknown command/event types
or unsupported schema versions receive explicit `protocol.error`, cause no side
effect, and close the connection. `protocol.error` carries no severity
discriminator: every protocol error is terminal for this connection, while the
host decides whether
an equivalent future connection may be attempted. When durable delivery is
implemented, unacknowledged outbox/journal messages will stay eligible for replay;
peers will not silently reinterpret unknown payloads.

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
server command outbox, daemon processed-command dedupe, daemon event buffering,
ACK-after-commit, reconnect reconciliation, session ownership, retry policy, and
Requirement transaction semantics. The current implementation provides the wire
and transport boundaries, daemon registration/authentication/liveness/revocation,
and the minimal session pinning/outbox foundation; daemon journals, full ACK/replay,
and business execution retry remain pending.

## Session routing and state ownership

The current session-routing flow selects a connected eligible daemon and persists
its identity before the first command. `DaemonRuntime::persist_and_dispatch_command`
constructs and dispatches the persisted envelope only through that owner, while
inbound events and ACKs from a different daemon receive a protocol error.
Reconnect receives one reconciliation
snapshot for the same identity; revocation leaves the session pinned. Full server
retry/failure handling remains pending.

The target durable-delivery flow is: Agent produces a readiness assessment →
daemon emits `requirement.assessed` → server will deduplicate and lock the
current Requirement, validate event revision and domain gates, persist typed
verdict, blockers, assumptions, and reviewed repository SHAs plus any valid
transition, commit, then send `event_ack(status=accepted)` (or commit a
rejection and send `event_ack(status=rejected)`). The daemon never writes
Requirement state directly.
