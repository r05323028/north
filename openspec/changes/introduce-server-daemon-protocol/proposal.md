# Introduce server-daemon protocol

## Why

Server and daemon need one boring, explicit contract: typed commands/events
with durable at-least-once delivery, stable idempotency identities, and
reconnect-safe ordering so duplicate frames cannot duplicate runtime or
business effects.

## What Changes

- `north-protocol` defines the envelope and baseline catalog: commands
  `session.start`, `session.cancel`, `session.resume`, `message.send`; events
  `session.started`, `agent.message`, `agent.activity`, `requirement.assessed`,
  `session.completed`, `session.failed`; and control/ACK/error frames.
- North 0.1 uses JSON text WebSocket messages. The server transport adapter is
  Axum WebSocket; the daemon transport adapter is `tokio-tungstenite`. Neither
  transport library is part of the North application wire contract.
- Transport adapters own upgrade/framing/ping/pong/close/socket lifecycle and
  bounded limits. North coordination, not either WebSocket library, owns
  durability, idempotency, ordering, replay, acknowledgement, and recovery.
- Server commands use durable outbox rows and daemon commands use a durable
  inbox/processed ledger. `command_ack(status=accepted)` means durable daemon receipt, not
  runtime completion; `event_ack(status=accepted)`/`event_ack(status=rejected)` follow server commit.
- Every direction has an independent monotonic per-session sequence:
  `server_command_seq` and `daemon_event_seq`. Reconnect reconciliation detects
  gaps and replays deterministically; ids remain idempotency keys.
- Hello/welcome negotiate exact protocol `0.1`, frame `schema_version: 1`, and
  fail closed with `protocol.error` for incompatible or unknown frames.

Repository preparation events stay out unless later changes prove genuine
protocol value. Cross-cutting semantics are canonical in
`harden-distributed-system-architecture`.

## Context assembly and typed evidence

`session.start` SHALL carry server-assembled `RequirementContext`, bounded
`ConversationContext`, and enabled repository metadata DTOs. Credentials,
checkout paths, persistence handles, and domain types stay out of the wire
crate. `requirement.assessed` SHALL carry typed readiness verdict/evidence;
`session.resume` SHALL contain execution recovery only and never a transport
sequence cursor. The canonical ACK wire names are `command_ack` and
`event_ack(status = accepted | rejected)`.

## Capabilities

### New Capabilities

- `daemon-protocol`: envelope shape, message catalog, durable bidirectional
  delivery, directional ordering, compatibility, and resume semantics.

### Modified Capabilities

(none)

## Impact

- `north-protocol` gains only serde/serde_json (allowed; still no runtime,
  transport, or business crates).
- `north-server` gains the minimal Axum WebSocket adapter; `north-daemon` gains
  Tokio + `tokio-tungstenite` supervisor dependencies with rustls native roots.
  The durable outbox/journal and credential persistence remain later work.

- Server/persistence later gain the command outbox and event transaction path;
  daemon later gains a local transport journal, not server database access.
- Affected docs: `docs/architecture/server-daemon-protocol.md` (canonical).
- Canonical cross-cutting contract: `harden-distributed-system-architecture`.
- Dependencies on earlier changes: introduce-daemon-runtime-connection.
