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
size, and bounded 256-item connection/inbound/outbound queues. The daemon
uses tokio-tungstenite with the minimal `rustls-tls-native-roots` feature for
WSS. These are configuration points, not reliability guarantees.

## Transport & topology

- Daemon-initiated persistent connection (WebSocket over TLS in deployment);
  the daemon may sit behind NAT/firewalls; no inbound ports.
- The server endpoint is an Axum upgrade handler plus a thin transport adapter;
  application/session coordination authenticates the first `hello`, owns
  registration, and receives decoded frames through bounded channels.
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
  daemon → server : hello/registration, heartbeat, command.accepted
  server → daemon : welcome, event.accepted, event.rejected,
                    reconciliation/watermarks, protocol.error

Server commands (server → daemon ONLY)
  session.start · session.cancel · session.resume · message.send

Daemon events (daemon → server ONLY)
  session.started · agent.message · agent.activity · requirement.assessed
  session.completed · session.failed
```

`command.accepted` means the daemon's durable inbox has recorded the command;
it is not runtime completion. `event.accepted` means the server committed the
business effect. `event.rejected` means the server durably recorded a
well-formed fact as rejected (for example stale assessment) and will not retry
its business effect. No success ACK is sent before its relevant commit.

`session.resume` is a server COMMAND that tells the daemon to resume work; it
is never a daemon→server event. Reconnect reconciliation uses control frames,
not a duplicated event name.

## Envelope and delivery contract

Every command/event carries:

- stable `command_id` or `event_id` for idempotency;
- `session_id` and a direction-specific sequence (`server_command_seq` or
  `daemon_event_seq`) for ordering and gap detection;
- `sent_at`, `type`, `payload`, and `schema_version`.

The server persists a command outbox row before dispatch. Retries reuse the
same `command_id` and sequence, and the outbox retains unaccepted commands.
The daemon persists a local command inbox/processed ledger before sending
`command.accepted`. Duplicate delivery repeats the known ACK and never invokes
the runtime twice. `message.send` is submitted to the agent at most once for
one command id, including reconnect and daemon restart recovery.

The daemon journals events before sending them. The server deduplicates event
ids inside the same transaction as validation/evidence/business state. The
server ACKs only after commit; unacknowledged daemon events remain buffered and
are replayed after reconnect.

## Sequence and reconnect rules

`server_command_seq` and `daemon_event_seq` are independent monotonic counters,
scoped to one session and direction. They start at 1 and are persisted with
the outbox/journal record. Reconciliation carries `ack_through_seq` plus a
sparse sequence set when processing is non-contiguous.

- A duplicate id+sequence is harmless and receives the known ACK again.
- One sequence with a different id is a protocol error.
- An out-of-order frame may be durably buffered, but is not applied until the
  missing sequence is replayed/reconciled.
- A late frame at or below an acknowledged contiguous sequence is inert.
- Buffered daemon events replay in ascending `daemon_event_seq`; server command
  retries retain ascending `server_command_seq`.
- Processed command rows may be compacted only after terminal session
  reconciliation proves a contiguous watermark. A durable per-session
  `processed_through_seq` tombstone remains, so old duplicates stay inert.

IDs answer “is this the same delivery?” Sequences answer “is this the next
ordered delivery?” Neither replaces the other.

## Compatibility and errors

A protocol-version mismatch receives `protocol.error(incompatible_protocol)`
and the connection closes before session traffic. Unknown command/event types
or unsupported schema versions receive explicit `protocol.error`, cause no side
effect, and close the connection. Unacknowledged outbox/journal messages stay
eligible for replay; peers do not silently reinterpret unknown payloads.

## Liveness and error boundaries

WebSocket ping/pong means the socket can exchange transport control frames. The
North `heartbeat` frame means an authenticated daemon is alive and reporting
application state. Ping/pong never proves durable session state, command
processing, or liveness after reconnect.

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
ACK-after-commit, reconnect reconciliation, session ownership, retry policy,
and Requirement transaction semantics.

## Session routing and state ownership

The server selects a connected eligible daemon and persists `session.daemon_id`
before the first command. Commands/events for a session are accepted only from
that daemon. Reconnect resumes against the same identity; North 0.1.0 performs
no live migration. If the daemon is unavailable, server retry/failure policy
handles the pinned session.

Agent produces a readiness assessment → daemon emits `requirement.assessed` →
server deduplicates and locks the current Requirement → validates event revision
and domain gates → persists assessment evidence and any valid transition →
commits → sends `event.accepted` (or commits a rejection and sends
`event.rejected`). The daemon never writes Requirement state directly.
