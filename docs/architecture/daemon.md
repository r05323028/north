# Daemon

The daemon runs locally near repositories and the agent runtime. It is an
**execution host**, not a business brain.

## Connection stack

The daemon uses `tokio-tungstenite` directly; no Socket.IO-style framework or
custom WebSocket implementation. One connection supervisor owns the lifecycle:

```text
Disconnected → Connecting → AwaitingWelcome → Authenticated
                                      ↓
                    Reconciling → ReconciliationReceived → Active
                                      ↓
                                  disconnect
                                      ↓
                         bounded transport backoff → reconnect
```

The supervisor sends `hello`, waits for `welcome`, receives one connection-level
reconciliation snapshot, delivers it to coordination, and waits for coordination
readiness before opening normal application traffic in `Active`. Runtime events and application heartbeat remain queued before `Active`; local
Journal replay uses the same gate;
ping/pong remains enabled as transport control. Handshake stages have
configurable hello, welcome, and reconciliation timeouts. Coordination uses one
whole-stage timeout covering event delivery, reconciliation application, and
readiness.
A healthy Active connection resets transport reconnect backoff.

Retryable socket/connect failures enter transport backoff. Protocol/schema
mismatch, rejected credentials, revoked identity, and protocol errors are terminal
and surface to the daemon host; they never reconnect forever.

Runtime/session coordination sends `north-protocol::DaemonFrame` values through
a bounded channel. Only the supervisor's single writer task converts them to
JSON text WebSocket messages. The supervisor delivers `HandshakeResult`
(welcome plus the complete reconciliation snapshot) through `ConnectionEvent`
and forwards application frames only after coordination signals readiness.
Each `SessionReconcileState` carries contiguous command/event ACK watermarks
plus a canonical `event_ack_sparse` list: strictly ascending, unique, and above
`event_ack_through_seq`. Transport validates wire shape; coordination owns meaning.
Ping/pong is transport liveness;
`heartbeat` is authenticated North application liveness.

Daemon status is Live only while the registration has an active connection and
its last North heartbeat is no older than 45 seconds. Known disconnects clear
connection state immediately; stale heartbeat state becomes Offline even when
transport close detection is delayed.

Transport defaults: 8 MiB message, 1 MiB frame, 256 outbound frames. Cargo
enables tokio-tungstenite's `rustls-tls-native-roots` feature for WSS; no
Socket.IO or native-tls stack is introduced.

## Responsibilities

Connection-supervisor, durable delivery, and WebSocket responsibilities below
are current where stated. Repository inspection remains a downstream contract.

- Initiate and maintain the server connection (WebSocket over TLS in deployment).
- The current authentication flow accepts one user-owned daemon registration,
  associates it with its configured identity and capabilities, updates
  heartbeat-based application liveness, and supports owner/admin revocation.
- Migrations 0007–0009 store setup requests, registrations, requirement-bound
  execution sessions, the server command outbox, and bounded verification-attempt
  state. Migration 0013 adds the configured repository catalog; migration 0014
  adds outbox payload fingerprints, command/event watermarks, and server event
  identity/outcome records.
  Plaintext credentials remain on daemon hosts; the server stores hashes only.
  Setup rows older than the retention window are removed opportunistically in
  bounded indexed batches.
- The `Journal` maintains a local transport journal: command
  inbox/processed-command ledger and unacknowledged event replay buffer. This is
  not a business database and never grants database access.
- The durable-delivery coordinator acknowledges a server command only after its
  inbox record is flushed durably (`command_ack`). This means durable receipt,
  not runtime completion.
- The coordinator crosses the local runtime seam once per `command_id`, passes
  that id as its operation id, and reattaches after restart when possible. It
  never re-invokes a `dispatch_started` command automatically.
- The repository-inspection design will manage a reusable repository cache plus
  isolated disposable session/task checkouts, detect dirty-checkout violations,
  and report exact commit SHAs.
- The durable coordinator converts runtime output into typed facts/events,
  journals them before transmission, replays them in `daemon_event_seq` order,
  and reports recoverability/failure. The shipped `LocalRuntime` is a
  placeholder, so executable commands currently surface a not-configured/
  unknown fact; the production agent adapter belongs to the downstream runtime
  change. Unknown outcomes emit an explicit `session.failed` fact without
  automatic resubmission.
- Reconnect the WebSocket with local backoff and replay eligible Journal buffers
  after reconciliation; execution recovery remains a server `session.resume`
  command.

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

The current session-routing foundation selects an eligible connected daemon in
`AuthStore::start_session_with_command`, assigns sequence metadata, serializes
one complete command envelope, and persists that exact payload atomically with
`execution_sessions.daemon_id`. `DaemonRuntime::persist_and_dispatch_command`
then dispatches the persisted envelope through its pinned owner. Reconnect
reconciles only sessions pinned
to that identity; North 0.1.0 does not perform automatic live migration. Due
business retries remain server-owned: a retry worker creates one durable
`session.resume` for the immutable owner, while reconnect only delivers existing
outbox work. Revoked owners are never replaced automatically.

The current registration model defines daemon registrations as instance-scoped
identities with credentials owned by the account recorded in `created_by`.
The credential owner can revoke its own credential, while Admin/Owner can revoke
any daemon credential. Revocation closes current access, refuses future
authentication, and keeps affected sessions pinned rather than migrating them.

## Failure posture

WebSocket reconnect/backoff and local Journal/runtime transport recovery are
current daemon mechanics. They do not consume the server's business attempt
budget. Event replay remains delivery recovery and does not consume that budget.

The server execution model persists `Idle`, `Running`, `Retrying`, or `Failed`
(and existing successful `Completed`) together with attempt identity/count,
snapshotted limit, persisted `next_retry_at`, and a bounded safe failure reason.
Only server policy decides `session.resume` and terminal `Failed`; a known retry
keeps the clarification run active and slot-occupying, while unknown outcome
never auto-resubmits. Requirement lifecycle remains separate.

WebSocket reconnect/backoff, journal replay, ACK retry, and event replay are
transport recovery and consume no business attempt. Setup request creation is
also subject to the public endpoint client bucket and transactional pending
quota on the persisted typed IPv4 `/24` or IPv6 `/64` network key; process-local
buckets reset on restart, while unexpired unclaimed setup rows remain durable.
Setup approval/claim credentials remain outside the browser response.

Setup/login follows the browser-assisted CLI flow. A normal browser GET
returns an HTML confirmation page with daemon label and state, an explicit
Approve POST form, and cancel/back navigation; `Accept: application/json`
retains the read-only JSON preview for programmatic clients. GET never mutates.
Only authenticated same-origin POST can approve, and an HTML POST returns a
human-readable success page. The daemon credential is returned only by the
separate claim endpoint polled by `north-daemon setup`; approval HTML contains
no credential or other claim secret. The CLI polls with bounded retries for
transient network and 5xx failures and stops on terminal errors or expiry.
Server startup clears prior daemon connection leases before accepting placement;
reconnect is required after a single-server restart. See
`docs/architecture/server-daemon-protocol.md` and change
`harden-daemon-runtime-correctness`.
