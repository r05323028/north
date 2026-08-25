# Daemon

The daemon runs locally near repositories and the agent runtime. It is an
**execution host**, not a business brain.

## Connection stack

The daemon uses `tokio-tungstenite` directly; no Socket.IO-style framework or
custom WebSocket implementation. One connection supervisor owns the lifecycle:

```text
connect → send hello/auth → receive welcome/reconcile → connected
       → reader task + single writer task → disconnect → bounded backoff → reconnect
```

Runtime/session coordination sends `north-protocol::DaemonFrame` values through
a bounded channel. Only the supervisor's writer task converts them to JSON text
WebSocket messages. The reader decodes server text messages and forwards
`ServerFrame` values independently. Ping/pong is transport liveness; `heartbeat`
is authenticated North application liveness.

Transport defaults: 8 MiB message, 1 MiB frame, 256 outbound frames. Cargo
enables tokio-tungstenite's `rustls-tls-native-roots` feature for WSS; no
Socket.IO or native-tls stack is introduced.

## Responsibilities

- Initiate and maintain the server connection (WebSocket over TLS in deployment).
- Authenticate one user-owned daemon registration and report identity,
  capabilities, and heartbeat liveness.
- Maintain a local durable transport journal: command inbox/processed-command
  ledger and unacknowledged event replay buffer. This is not a business
  database and never grants database access.
- Acknowledge a server command only after its inbox record is flushed durably
  (`command.accepted`). This means durable receipt, not runtime completion.
- Invoke the local runtime once per `command_id`; pass that id as its operation
  id and reattach after restart when possible. Never re-invoke a
  `dispatch_started` command automatically.
- Manage a reusable repository cache plus unique disposable session/task
  checkouts; report dirty-checkout violations and exact commit SHAs.
- Convert runtime output into typed facts/events, replay them in
  `daemon_event_seq` order, and report recoverability/failure.
- Reconnect the WebSocket with local backoff and resume transport buffers when
  instructed by the server.

## Non-responsibilities

- No Requirement lifecycle or readiness decisions; no direct database access;
  no `north-domain`, `north-persistence`, or `north-server` dependency.
- No server execution state, business retry budget, or decision that work is
  permanently `Failed`. `session.failed` is a fact report; server policy owns
  the state transition.
- No daemon migration of a session to another daemon.
- No repository credentials sent to or stored by the server. Host Git config,
  credential helpers, and SSH agent remain local.

## Ownership and reconnect

The server selects and persists `session.daemon_id` before the first command.
Every command/event is authenticated and routed against that identity. A
reconnect may reconcile only sessions pinned to that daemon; North 0.1.0 does
not perform automatic live migration. If the daemon is unavailable, the server
applies its persisted execution retry policy while retaining ownership.

Daemon registrations are instance-scoped identities with credentials owned by
the account recorded in `created_by`. The owner may revoke its own credential;
Admin/Owner may revoke any. Revocation closes current access and refuses future
connections; pinned sessions remain pinned and follow normal retry/failure
handling.

## Failure posture

WebSocket reconnect/backoff, event replay, and local runtime transport recovery
are daemon mechanics. They do not consume the server's business attempt
budget. The server persists `Idle`, `Running`, `Retrying`, or `Failed`, the
attempt count, budget, and reason; only the server decides when to send
`session.resume` and when exhaustion becomes `Failed`. Execution failure never
mutates Requirement lifecycle state.

Setup/login follows the browser-assisted CLI flow: see
`docs/architecture/server-daemon-protocol.md` and change
`introduce-daemon-runtime-connection`.
