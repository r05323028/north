# Design

## Context

Wire format must survive reconnects, retries, and future evolution without
leaking business types across crates. The cross-cutting choices are defined in
`harden-distributed-system-architecture`; this design fixes the protocol
implementation choices that must match it.

## Decisions

- North 0.1 uses JSON text frames: one serialized `north-protocol` value per
  WebSocket text message. Binary WebSocket messages are rejected at adapters.
  The protocol crate exposes no Axum, Tokio, Tungstenite, or WebSocket types.
- Server transport is an Axum upgrade handler plus a thin adapter; it starts
  the hello deadline immediately after upgrade, reads hello before bounded
  coordinator admission, and applies a separate admission timeout. Daemon
  transport is one `tokio-tungstenite` connection supervisor with a bounded
  outbound channel, single writer task, reader task, and local reconnect
  backoff. No Socket.IO or custom framing is introduced.
- Envelope uses explicit direction-specific fields: `command_id` plus
  `server_command_seq` for server commands, `event_id` plus
  `daemon_event_seq` for daemon events, `session_id`, `sent_at`, `type`,
  `payload`, and `schema_version`. Hello/welcome carry exact
  `protocol_version: "0.1"`.
- Commands, events, and control frames are disjoint exhaustive enums in
  `north-protocol`. `session.resume` is a server command only.
- Server command rows are persisted before dispatch and retried with the same
  id/sequence until daemon `command_ack`. Daemon command inbox and event
  replay use a flushed local append-only journal. A duplicate command returns
  its known ACK and never invokes the runtime twice.
- Server event handling deduplicates inside the same transaction as validation,
  immutable evidence, and any business transition. `event_ack(status=accepted)` follows a
  committed effect; `event_ack(status=rejected)` follows a committed durable rejection
  record for a well-formed fact that cannot apply. No ACK follows a rollback.
- Reconciliation is one finite connection-level `ReconcileSnapshot` with zero
  or more unique `SessionReconcileState` entries. Each entry carries contiguous
  command/event watermarks plus a strictly ascending, unique `event_ack_sparse`
  list whose values are above `event_ack_through_seq`.
  Valid out-of-order frames can be buffered but are not applied until gaps close.
  Same id+sequence is harmless; same sequence with another id is a protocol error.
- Unknown frame types and unsupported schema versions receive
  `protocol.error`, cause no side effect, and close the connection. The error
  frame has no severity flag: every protocol error is terminal to the current
  connection; the host decides whether a future connection may be attempted.
  Version mismatch is rejected before session traffic. No plugin/range negotiation.
- The daemon emits a typed handshake result containing `Welcome` and
  `ReconcileSnapshot` to coordination, then enters `Active` only after
  coordination applies/restores replay state and signals readiness.
- The protocol crate remains serde-only; domain conversions, durable outbox,
  runtime idempotency, and persistence transactions live in hosts.

## Context DTO and validation decisions

- `SessionStart` carries `RequirementContext`, a bounded/relevant
  `ConversationContext` excerpt, and enabled `RepositoryContext` metadata.
  `north-server` assembles and converts snapshots; `north-protocol` owns only
  transport DTO validation. Credentials, checkout paths, and domain types do
  not cross the boundary.
- `RequirementAssessed` carries `ReadinessVerdictWire`, blockers, assumptions,
  and `ReviewedRepositoryWire { repository_id, commit_sha }`; it never embeds
  a serialized domain assessment string. Server/domain conversion remains
  explicit and server-authoritative.
- `SessionResume` is an empty execution-recovery command in 0.1.0. Transport
  replay cursors live only in `ReconcileSnapshot` watermarks and canonical
  `event_ack_sparse` lists.

## Risks / Trade-offs

- **Crash after a command is marked dispatch-started** → runtime operation id
  is the command id; reattach when possible, otherwise report explicit unknown
  outcome and do not automatically resubmit side-effecting commands.
- **Journal growth** → compact command rows only to a durable per-session
  processed sequence watermark; retain the watermark for the session.
- **Protocol evolution is deliberately strict** → deploy matching 0.1.x peers;
  incompatible peers fail closed instead of silently corrupting state.
