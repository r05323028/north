# Server ↔ daemon protocol

Canonical message catalog: `crates/north-protocol/src/lib.rs` + OpenSpec change
`introduce-server-daemon-protocol`. This doc fixes the contract shape.

## Transport & topology

- Daemon-initiated persistent connection (WebSocket over TLS in deployment);
  the daemon may sit behind NAT/firewalls; no inbound ports.
- Daemon authenticates with a locally stored CLI/daemon credential obtained via
  the Multica-like browser login (`north setup --server-url …`) — never a reused
  email verification code.
- After auth the daemon registers identity + capabilities; heartbeat maintains liveness.

## Envelope contract (invariants)

- Every command/event carries stable ids: `command_id` / `event_id` / `session_id`.
- Delivery is at-least-once; processing is idempotent. Duplicates must not
  duplicate durable effects. Reconnect/resume is designed around this.
- Commands/events tolerate retries and late arrival after reconnect.

## Baseline messages (0.1.0)

Server → Daemon: `session.start`, `session.cancel`, `session.resume`, `message.send`.
Daemon → Server: `session.started`, `agent.message`, `agent.activity`,
`requirement.assessed`, `session.completed`, `session.failed`.

Repository preparation events may be added only if they earn protocol value.
Internal runtime/tool operations are NOT public protocol.

## State ownership example

Agent produces a readiness assessment → daemon emits `requirement.assessed` →
server validates revision + invariants → persistence updates → Discussing→Ready.
The daemon never writes requirement state directly.
