# Introduce daemon registration and connection

## Why

Agent runtimes live on developer machines behind NAT. A daemon-initiated
connection with browser-assisted CLI login lets a user-owned machine join
without inbound ports or a pairing bureaucracy, while sessions still need an
explicit durable owner when multiple daemons exist.

## What Changes

- `north setup --server-url …`: browser completes normal North login, then the
  server issues a dedicated user-owned CLI/daemon credential stored locally
  (never a reused verification code).
- Daemon connects outbound using one `tokio-tungstenite` connection
  supervisor (persistent WebSocket over TLS), authenticates with the local
  credential, registers one durable daemon identity + capabilities, and reports
  heartbeat/liveness. The server endpoint is an Axum WebSocket adapter.
- The supervisor owns hello, reader/writer tasks, ping/pong, bounded outbound
  buffering, disconnect, and transport backoff/reconnect; session/runtime code
  does not own a second reconnect loop. Normal traffic starts only after
  `welcome` and reconciliation; coordination applies the reconciliation snapshot
  before Active; retryable transport failures back off while protocol/auth
  failures surface terminally without reconnect loops.
- The server selects an eligible daemon before the first command and persists
  `session.daemon_id`; reconnect resumes only sessions pinned to that identity.
  North 0.1.0 performs no automatic live migration or multi-user sharing.
- Credential owner may revoke its credential; Admin/Owner may revoke any.
  Revocation closes live access, refuses future handshakes, and leaves pinned
  sessions to server retry/failure handling.
- Settings > Daemon Status shows connected/offline daemons without runtime
  internals.

Out of scope: manual pairing codes, transferable credentials, multi-user
sharing, live session migration, and remote daemon administration beyond
revocation/status.

## Capabilities

### New Capabilities

- `daemon-runtime`: setup/login flow, user-owned credential model, daemon
  identity, connection lifecycle, capability registration, liveness, and
  session-owner routing.

### Modified Capabilities

(none)

## Impact

- New crate code in north-daemon (connection client); server gains WS endpoint
  (axum upgrade) and durable daemon/session ownership fields.
- Affected docs: docs/architecture/daemon.md,
  docs/architecture/server-daemon-protocol.md, and the canonical
  `harden-distributed-system-architecture` ownership contract.
- Dependencies on earlier changes: introduce-email-auth-and-owner-bootstrap
  (login), introduce-role-and-permission-model (revocation is admin-gated).
