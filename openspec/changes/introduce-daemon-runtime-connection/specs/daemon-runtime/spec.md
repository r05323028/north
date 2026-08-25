## Purpose

Lets any developer machine become a North execution host through outbound-only
connectivity and a one-time browser login — no inbound ports, no pairing codes.

## ADDED Requirements

### Requirement: Browser-based CLI login issues a daemon credential

Running the setup flow against a server URL SHALL let the user authenticate in
their browser and receive a dedicated daemon credential. This credential SHALL
be distinct from email verification codes, SHALL be shown once, and SHALL be
stored locally by the CLI with owner-only file permissions.

#### Scenario: Setup completes without inbound access

- **WHEN** a user runs setup from a NAT-ed machine and approves in browser
- **THEN** the local machine holds a working daemon credential and the server
opened no inbound port

### Requirement: Daemon-initiated persistent connection

Daemons SHALL always initiate the connection (outbound WebSocket over TLS in
deployment). After authenticating, the daemon SHALL register identity and
capabilities; the server SHALL track liveness via heartbeats (last_seen_at).

#### Scenario: Behind firewall still connects

- **WHEN** a daemon starts on a host with no reachable inbound ports
- **THEN** it connects, registers, and appears live in daemon status

### Requirement: Revocation cuts access

Admins/Owners SHALL revoke a daemon credential; subsequent connection
attempts with it SHALL fail.

#### Scenario: Stolen laptop scenario

- **WHEN** an admin revokes a daemon's credential
- **THEN** its next connect attempt is refused and current connection drops

### Requirement: Offline is informational

A daemon going offline SHALL change only liveness status (last_seen_at stops
updating). It MUST NOT alter requirement lifecycle states.

#### Scenario: Machine sleeps mid-instance

- **WHEN** the daemon disappears
- **THEN** requirements keep their statuses unchanged; UI shows daemon offline
