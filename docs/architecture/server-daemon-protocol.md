# Server ↔ daemon protocol

Canonical message catalog: `crates/north-protocol/src/lib.rs` + OpenSpec change
`introduce-server-daemon-protocol`. This doc fixes the contract shape.

## Transport & topology

- Daemon-initiated persistent connection (WebSocket over TLS in deployment);
  the daemon may sit behind NAT/firewalls; no inbound ports.
- Daemon authenticates with a locally stored CLI/daemon credential obtained via
  the Multica-like browser login (`north setup --server-url …`) — never a reused
  email verification code.
- After auth the daemon registers identity + capabilities; heartbeats maintain liveness.

## Frame groups — direction is part of a message's identity

Three disjoint groups. A message belongs to exactly one group; nothing appears
in both directions under the same name.

```text
Connection/control frames (connection-scoped)
  daemon → server : hello/registration (identity, capabilities)
  daemon → server : heartbeat (liveness)
  server → daemon : acknowledgement(processed event_ids)
  server → daemon : resume/reconciliation state (session continuity data)

Server commands (server → daemon ONLY)
  session.start · session.cancel · session.resume · message.send

Daemon events (daemon → server ONLY)
  session.started · agent.message · agent.activity · requirement.assessed
  session.completed · session.failed
```

`session.resume` is a server COMMAND that tells the daemon to resume work; it
is never a daemon→server event. Reconnect reconciliation uses the control
frames above, not a duplicated event name.

## Envelope contract (invariants)

- Every command/event carries stable ids: `command_id` / `event_id` / `session_id`.
- Delivery is at-least-once; processing is idempotent. Duplicates must not
  duplicate durable effects.
- **Acknowledgement gap is closed by design**: the daemon buffers events until
  the server acknowledges their ids (control frame); only acknowledged ids are
  trimmed from the replay buffer. On reconnect, unacknowledged events are
  safely resent in order.
- Commands/events tolerate retries and late arrival after reconnect.

## State ownership example

Agent produces a readiness assessment → daemon emits `requirement.assessed` →
server validates revision + invariants → persistence updates → Discussing→Ready,
then the server acknowledges the event id. The daemon never writes requirement
state directly.
