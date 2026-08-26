# Design

## Context

Multica-style CLI login lets a local daemon dial out, but a connected daemon is
not interchangeable with another daemon once a session has started. The
cross-cutting ownership contract lives in
`harden-distributed-system-architecture`.

## Decisions

- The server exposes an Axum WebSocket upgrade and keeps the handler thin. It
  forwards decoded `north-protocol` frames through bounded connection channels;
  the coordinator authenticates the first `hello`, registers the daemon, and
  sends `welcome` plus one connection-level reconciliation snapshot.
- The daemon uses one `tokio-tungstenite` supervisor with a single writer and
  independent reader. It sends hello on each connection, handles WebSocket
  ping/pong as transport control, forwards JSON text frames to coordination, and
  applies only local bounded reconnect backoff. North execution retry budgets
  remain server-owned.
- The daemon supervisor uses explicit phases `Connecting → AwaitingWelcome →
  Authenticated → Reconciling → ReconciliationReceived → Active`. It does not
  drain normal outbound frames, replay local events, or send application
  heartbeat before `Active`; WebSocket ping/pong remains available in every
  phase. The connection-level reconciliation snapshot is delivered to
  coordination, which signals readiness after applying/restoring replay state.
- Handshake timeouts for hello, welcome/authentication, reconciliation, and
  coordination readiness are configuration, separate from execution timeout and
  server retry budget.
- `ConnectionError` classifies socket/connect interruption and handshake timeout
  as retryable, while malformed/unsupported protocol, authentication/revocation,
  and `protocol.error` as terminal. Every protocol error closes the connection;
  terminal failures return to the host and stop automatic reconnect.
- `north setup --server-url <url>` uses a short-lived browser request token;
  approval returns one random secret shown once and stored by the CLI with
  owner-only permissions. Email verification codes are never reused.
- A credential record stores `daemon_id`, label, secret hash, `created_by`,
  created_at, and revoked_at. The daemon registration is instance-scoped; the
  credential is owned by the account in `created_by` and is not transferable
  or shared in 0.1.0. The owner may revoke its own credential; Admin/Owner may
  revoke any.
- The authenticated connection registers the durable daemon identity,
  capabilities, and protocol version. Heartbeats update `last_seen_at`.
  Protocol compatibility and command/event reconciliation follow the protocol
  hardening contract.
- Session start performs a simple connected/capability/repository filter,
  persists `session.daemon_id` atomically with the first command outbox row,
  and routes all later frames to that identity. No scheduler abstraction.
- A reconnect with the same unrevoked identity receives one connection-level
  reconciliation snapshot for zero or more pinned sessions; coordination applies
  it before Active. Another daemon cannot claim those sessions. Revocation closes
  current connections and causes pinned sessions to follow server retry/failure
  policy without migration.
- Server-side liveness is informational and never directly changes Requirement
  lifecycle state.

## Risks / Trade-offs

- **A daemon outage strands pinned work** → server retry/failure state is
  explicit; new work can choose another daemon, but live migration is deferred.
- **Credential ownership limits sharing** → this preserves the existing 0.1.0
  out-of-scope decision; a future sharing model can add explicit grants.
- **Capability selection is intentionally simple** → filter at session start;
  defer scheduler frameworks until real placement requirements exist.
