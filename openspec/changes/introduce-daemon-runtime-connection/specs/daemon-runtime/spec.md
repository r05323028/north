## Purpose

Lets a user-owned developer machine become a North execution host through outbound-only connectivity, explicit session ownership, and a one-time browser login.

## ADDED Requirements

### Requirement: Browser-based CLI login issues a user-owned daemon credential

Running setup against a server URL SHALL let an account authenticate in its
browser and receive a dedicated daemon credential. The credential SHALL be
distinct from email verification codes, shown once, stored locally with
owner-only file permissions, and durably attributed by `created_by` to the
account that authorized setup. Credentials SHALL not be transferable or shared
between users in 0.1.0.

#### Scenario: Setup completes without inbound access

- **WHEN** a user runs setup from a NAT-ed machine and approves in browser
- **THEN** the local machine holds a working credential and the server opened no inbound port

### Requirement: Daemon-initiated persistent connection

Daemons SHALL always initiate the connection using the `tokio-tungstenite`
supervisor (outbound WebSocket over TLS in deployment). The server SHALL accept
it through an Axum WebSocket upgrade/transport adapter. After authenticating,
the daemon SHALL register a durable daemon identity, protocol version, and
capabilities; the server SHALL track liveness via North heartbeats
(`last_seen_at`). WebSocket ping/pong SHALL NOT replace that heartbeat.

#### Scenario: Behind firewall still connects

- **WHEN** a daemon starts on a host with no reachable inbound ports
- **THEN** it connects, registers, and appears live in daemon status

### Requirement: Connection handshake gates application traffic

The daemon SHALL use one supervisor with explicit phases
`Connecting`, `AwaitingWelcome`, `Authenticated`, `Reconciling`,
`ReconciliationReceived`, and `Active`. It SHALL send `hello`, wait for
`welcome`, receive one connection-level reconciliation snapshot, deliver it to
coordination, and wait for coordination readiness before transmitting ordinary
commands, events, replayed journal frames, or North heartbeat. WebSocket
ping/pong SHALL remain available before `Active`. Handshake hello,
welcome/authentication, reconciliation, and coordination stages SHALL have
configurable timeouts distinct from execution retry policy.

#### Scenario: Runtime traffic cannot race authentication

- **WHEN** a daemon has sent hello but has not received welcome and reconciliation
- **THEN** normal application frames remain queued and are not written to the WebSocket

#### Scenario: Reconciliation receipt does not activate transport

- **WHEN** the daemon receives a valid reconciliation snapshot but coordination has not signaled readiness
- **THEN** normal application frames remain queued until coordination completes reconciliation

### Requirement: Terminal protocol failures stop reconnect

Retryable socket/connect interruption and temporary peer absence MAY enter
transport backoff. Unsupported protocol/schema, authentication or credential
revocation failure, invalid daemon identity, `protocol.error`, and
non-recoverable reconciliation violations SHALL surface as terminal connection
errors and SHALL NOT reconnect automatically. A `protocol.error` has no severity
discriminator: receiving one always closes the current connection.

#### Scenario: Protocol error does not loop

- **WHEN** the server sends `protocol.error`
- **THEN** the supervisor returns a terminal error to the daemon host without another reconnect attempt

### Requirement: Healthy connection resets transport backoff

After coordination signals readiness and the supervisor enters `Active`, the
transport reconnect attempt counter SHALL reset to its initial state. A later
retryable disconnect SHALL therefore use the initial backoff delay. This state is
independent from server execution retry budgets.

#### Scenario: Healthy connection clears historical failures

- **WHEN** several transport attempts fail, a connection becomes `Active`, and it later disconnects
- **THEN** the next retry starts at the initial transport backoff delay

### Requirement: Active sessions are pinned to their daemon

Before the first command for an active session, the server SHALL select an
eligible daemon and persist `session.daemon_id`. Every command/event for that
session SHALL be routed and authorized against that identity. A reconnect from
the same identity MAY resume the session; another daemon SHALL NOT claim it.
North 0.1.0 SHALL perform no automatic live migration.

#### Scenario: Reconnect uses the same owner

- **WHEN** daemon D1 reconnects after a connection loss for a session pinned to D1
- **THEN** D1 may reconcile that session and daemon D2 cannot receive its commands

### Requirement: Revocation cuts access without migration

The credential owner MAY revoke its own credential, and Admin/Owner SHALL be
able to revoke any credential. Revocation SHALL close current connections and
refuse subsequent handshakes. Sessions pinned to the revoked daemon SHALL
remain pinned and follow server retry/failure semantics rather than silently
moving to another daemon.

#### Scenario: Stolen laptop scenario

- **WHEN** an Admin revokes daemon D1's credential
- **THEN** D1's current connection drops, its next connect fails, and its sessions are not reassigned

### Requirement: Offline is informational to Requirement state

A daemon going offline SHALL stop liveness updates and may trigger server-owned
execution retry handling, but it MUST NOT directly alter Requirement lifecycle
state.

#### Scenario: Machine sleeps mid-session

- **WHEN** the pinned daemon disappears
- **THEN** the session remains owned by that daemon, the server applies retry/failure policy, and Requirement status/revision stay unchanged
