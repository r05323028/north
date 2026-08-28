# Introduce server-daemon protocol

## Why

Server and daemon need one boring, explicit contract: typed commands/events
with durable at-least-once delivery, stable idempotency identities, and
reconnect-safe ordering so duplicate frames cannot duplicate runtime or
business effects.

## Current state and ownership

The following foundation is already implemented and remains a prerequisite,
not pending scope:

- `north-protocol` has serde-only typed command, event, ACK, handshake, and
  reconciliation values plus JSON validation.
- North 0.1 uses JSON text WebSocket messages, an Axum server adapter, and a
  `tokio-tungstenite` daemon connection supervisor.
- The handshake receives exact protocol/schema versions, a `Welcome`, and one
  finite `ReconcileSnapshot`; daemon application traffic waits for coordination
  readiness.
- Server-owned session context assembly, typed readiness evidence, revision and
  state-version gates, daemon identity/authentication, liveness, revocation, and
  session pinning are established by earlier changes and canonical specs.
- The current server command-outbox/session-start path persists one complete
  command envelope before dispatch, but it is not the complete durable-delivery
  implementation below.

This active change still owns:

- the complete server command outbox ACK/resend lifecycle;
- the daemon command inbox/processed journal and recovery state machine;
- daemon event journaling and replay;
- durable `command_ack` and `event_ack` handling;
- atomic per-session directional sequence allocation;
- identity/payload conflict detection, bounded gap handling, and reconciliation;
- crash/restart idempotency proofs and safe compaction/retention.

No item above is implied by WebSocket delivery or transport reconnect. North
coordination remains responsible for it.

## What Changes

- `north-protocol` defines the baseline catalog: commands `session.start`,
  `session.cancel`, `session.resume`, and `message.send`; events
  `session.started`, `agent.message`, `agent.activity`, `requirement.assessed`,
  `session.completed`, and `session.failed`; plus control, ACK, reconciliation,
  and protocol-error frames.
- North 0.1 uses JSON text WebSocket messages. The server transport adapter is
  Axum WebSocket; the daemon transport adapter is `tokio-tungstenite`. Transport
  libraries provide transport only.
- `protocol.error` is a bidirectional connection/control frame. Server-to-daemon
  errors report daemon protocol violations; daemon-to-server errors report
  server protocol violations. Every received or emitted protocol error is
  terminal to the current connection and has no severity field.
- `CommandEnvelope` and `EventEnvelope` have different required fields and
  independent per-session sequence spaces. Connection/control frames use their
  own schemas and are not forced to carry `session_id`.
- `command_id` and `event_id` are globally unique opaque delivery identities;
  sequence uniqueness is scoped to `(session_id, direction)` and is persisted
  with the corresponding durable record.
- Server commands are durably outboxed before dispatch. Daemon commands move
  through durable `received`, `dispatch_started`, and `terminal` states. A
  `command_ack` means durable receipt for processing, not runtime completion.
- Daemon events are journaled before transmission. The server commits either a
  business effect or a durable rejection before sending
  `event_ack(status=accepted)` or `event_ack(status=rejected)`; both are
  terminal transport acknowledgements for that exact event.
- Reconciliation uses one finite connection-level snapshot with one state per
  pinned session. The server resends unacknowledged commands in sequence order;
  the daemon replays unacknowledged events in sequence order after applying the
  snapshot's watermarks and sparse ACKs.
- A crash after dispatch begins never causes blind resubmission of a
  side-effecting operation. Recovery first reattaches by stable operation
  identity; if outcome remains unknowable, the daemon records terminal unknown
  state and emits an execution `session.failed` fact with
  `recoverable: false`.

Repository preparation events stay out unless a later change proves genuine
protocol value. The protocol does not own agent prompting, clarification
behavior, tool choice, Requirement readiness judgment, repository inspection,
or server business retry policy.

## Context assembly and typed evidence

`session.start` SHALL carry server-assembled `RequirementContext`, bounded and
relevant `ConversationContext`, and enabled repository metadata DTOs. The wire
boundary contains no credentials, checkout paths, persistence handles, or domain
objects. `requirement.assessed` carries typed readiness evidence. The server
still owns conversion and the transaction that compares `revision`,
`state_version`, `assessment_id`, and `accepted_state_version`; the protocol
only durably delivers the fact.

`session.resume` remains an execution-recovery command only. It does not carry a
transport cursor; replay cursors live in reconciliation state.

## Capabilities

### New Capabilities

- `daemon-protocol`: envelope shape, message catalog, durable bidirectional
  delivery, directional ordering, compatibility, and reconnect semantics.

### Modified Capabilities

(none)

## Impact

- `north-protocol` remains serde/serde_json-only. Its wire enums gain the
  bidirectional `protocol.error` variant and round-trip coverage; no business or
  transport types enter the crate.
- `north-server` owns durable command outbox and event transaction integration;
  `north-daemon` owns its local command/event journal and recovery. Neither
  crosses the other host's persistence boundary.
- Affected canonical doc: `docs/architecture/server-daemon-protocol.md`.
- Established prerequisites: daemon runtime connection, distributed
  architecture guardrails, and the existing role/readiness contracts. Earlier
  P0 prerequisite changes are complete; their canonical specs are the source of
  truth. The enabled repository catalog is supplied by the active
  `introduce-configured-repositories` change; this protocol change only carries
  its server-assembled metadata DTO.
- This OpenSpec change does not implement agent runtime behavior, repository
  inspection, business retry policy, or Requirement lifecycle policy.

## Validation gate

Implementation is not complete until focused journal/restart/fault-injection
integration tests, the PostgreSQL-backed server tests, Rust validation, web
validation, and `openspec validate --all --strict` pass. The validation gate
must not mark an unimplemented durable-delivery task complete.
