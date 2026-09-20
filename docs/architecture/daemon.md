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

Connection-supervisor, durable delivery, WebSocket, and repository-inspection
responsibilities below describe current mechanics. Business retry remains a
specified downstream contract.

- Initiate and maintain the server connection (WebSocket over TLS in deployment).
- The current authentication flow accepts one user-owned daemon registration,
  associates it with its configured identity and capabilities, updates
  heartbeat-based application liveness, and supports owner/admin revocation.
- Migrations 0007–0009 store setup requests, registrations, requirement-bound
  execution sessions, the server command outbox, and bounded verification-attempt
  state. Migration 0013 adds the configured repository catalog; migration 0014
  adds outbox payload fingerprints, command/event watermarks, and server event
  identity/outcome records. Migration 0015 adds clarification-run context,
  cancellation/runtime outcome fields, and coarse activity records.
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
- The repository-inspection implementation manages a reusable repository cache
  plus isolated disposable session/task checkouts, detects dirty-checkout
  violations, and reports exact commit SHAs.
- The durable coordinator converts runtime output into typed facts/events,
  journals them before transmission, replays them in `daemon_event_seq` order,
  and reports recoverability/failure. The shipped `north-daemon` wires the
  landed `PiClarificationAdapter` backed by Pi Agent; Pi-specific details stay
  behind the daemon-private North runtime seam. Unknown outcomes emit an
  explicit `session.failed` fact without automatic resubmission.
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
to that identity; North 0.1.0 does not perform automatic live migration. Business retry policy remains server-owned. The durable retry worker polls due rows with database row locks, creates one pinned `session.resume` attempt atomically with its outbox row and counter, and leaves delivery to reconnect/replay. Revoked owners are never replaced automatically.

The current registration model defines daemon registrations as instance-scoped
identities with credentials owned by the account recorded in `created_by`.
The credential owner can revoke its own credential, while Admin/Owner can revoke
any daemon credential. Revocation closes current access, refuses future
authentication, and keeps affected sessions pinned rather than migrating them.

## Failure posture

WebSocket reconnect/backoff and local Journal/runtime transport recovery are
current daemon mechanics. They do not consume the server's business attempt
budget. Event replay remains delivery recovery and does not consume that budget.

The clarification runtime persists `Idle`, `Running`, `Retrying`, `Completed`,
or terminal `Failed`. `session.failed` closes the current attempt and the server
chooses bounded retry, exhaustion, unknown-outcome, owner-unavailable, or
cancellation policy without mutating Requirement lifecycle. `Retrying` keeps the
logical run and sequential slot active until a durable due `session.resume` is
created or policy terminalizes it. Attempt identity/count, snapshotted limits,
`next_retry_at`, safe failure classification, and current-attempt clearing are
server-owned persistence state; daemon reconnect and journal replay remain
transport recovery.

WebSocket reconnect/backoff, journal replay, ACK retry, and event replay are
transport recovery and consume no business attempt. Public endpoint abuse
protection is **Enforced** for `POST /auth/request-code` and
`POST /daemon/setup/request`: the server resolves a normalized effective client
address, derives an IPv4 `/32` or IPv6 `/64` primary CIDR key, and applies
endpoint-isolated process-local buckets with capacity 5 and one token per 120
seconds. Trusted `X-Forwarded-For` is opt-in by immediate-peer CIDR. Process
buckets reset on restart; migration 0018 persists the same CIDR as each new
setup row's durable quota key and limits three unexpired, unclaimed rows.
All limiter/quota rejection responses are generic 429s; PostgreSQL concurrency
proof runs with `NORTH_TEST_DATABASE_URL`. Setup approval/claim credentials remain outside the browser response. A
valid request consumes its client token before resource-specific checks; cooldown
and pending-quota rejection do not refund it, while malformed input consumes no
token. Abuse-control telemetry contains only typed endpoint, outcome, and coarse
category labels. LogCodeDelivery remains a separate configured delivery boundary
for development/self-hosted verification-code output. Production serving MUST
use into_make_service_with_connect_info::<SocketAddr>(); a plain Axum service
without ConnectInfo<SocketAddr> cannot resolve the immediate peer for these
public creation routes.

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
