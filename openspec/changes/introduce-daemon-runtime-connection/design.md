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
- Handshake timeouts for hello, welcome/authentication, and reconciliation are
  configuration. Coordination readiness has one total stage budget covering
  `HandshakeComplete` delivery, reconciliation application, and the readiness
  signal. All remain separate from execution timeout and server retry budget.
- `ConnectionError` classifies socket/connect interruption and handshake timeout
  as retryable, while malformed/unsupported protocol, authentication/revocation,
  and `protocol.error` as terminal. Every protocol error closes the connection;
  terminal failures return to the host and stop automatic reconnect.
- `north setup --server-url <url>` creates a short-lived device request. The
  browser approves it after normal login; CLI polling atomically claims one
  generated secret, which is shown only to the CLI and stored with owner-only
  permissions. Email verification codes are never reused.
- Migration 0007 stores short-lived setup requests, daemon registrations, and
  the minimal execution session/command-outbox rows. A credential record stores
  `daemon_id`, label, secret hash, `created_by`, created_at, revoked_at, and
  liveness fields. Plaintext request tokens and credentials are never persisted.
  The daemon registration is instance-scoped; the credential is owned by the
  account in `created_by` and is not transferable or shared in 0.1.0. The owner
  may revoke its own credential; Admin/Owner may revoke any.
- The authenticated connection registers the durable daemon identity,
  capabilities, and protocol version. Heartbeats update `last_seen_at`.
  Protocol compatibility and command/event reconciliation follow the protocol
  hardening contract.
- Session start performs a simple connected/capability/repository filter,
  persists `session.daemon_id` atomically with the first command outbox row in
  the minimal execution-session tables, and routes all later frames to that
  identity. No scheduler abstraction.
- A reconnect with the same unrevoked identity receives one connection-level
  reconciliation snapshot for zero or more pinned sessions; coordination applies
  it before Active. Another daemon cannot claim those sessions. Revocation closes
  current connections and causes pinned sessions to follow server retry/failure
  policy without migration.
- Server-side liveness is informational and never directly changes Requirement
  lifecycle state.

## Verification remediation decisions

- Daemon coordination consumes the connection-level reconciliation snapshot into
  its session state before sending the readiness signal. Post-handshake server
  frames are handed to the same coordination seam; unsupported duplicate
  handshake frames fail rather than being silently discarded.
- Each authenticated inbound application frame revalidates the daemon's
  connection ID and unrevoked registration before identity/session handling.
  Revocation therefore invalidates frame processing as well as closing the live
  socket.
- Admin/Owner users can view all daemon registrations. Other authenticated users
  can view only registrations whose `created_by` matches their user ID; the
  existing owner/admin revoke policy remains unchanged.
- Daemon status and new-session eligibility use the same liveness rule: Live
  only while `connected_at` is present and `last_seen_at` is within 45 seconds. Known disconnects still clear connection
  state immediately; stale heartbeats become Offline without requiring socket
  close detection.
- Server transport config owns only hello and bounded coordinator admission.
  Daemon-side config owns welcome, reconciliation, and coordination readiness
  deadlines; duplicate server knobs are removed.
- The DB-backed daemon lifecycle test remains explicitly ignored in ordinary
  no-database local runs, but CI provisions PostgreSQL and executes it in a
  dedicated required integration job. Local verification uses the same command
  with `NORTH_TEST_DATABASE_URL` set.

## Risks / Trade-offs

- **A daemon outage strands pinned work** → server retry/failure state is
  explicit; new work can choose another daemon, but live migration is deferred.
- **Credential ownership limits sharing** → this preserves the existing 0.1.0
  out-of-scope decision; a future sharing model can add explicit grants.
- **Capability selection is intentionally simple** → filter at session start;
  defer scheduler frameworks until real placement requirements exist.
