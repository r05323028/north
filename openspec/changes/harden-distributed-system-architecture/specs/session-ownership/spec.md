## Purpose

Pins execution sessions to explicit daemon identities and defines credential ownership, selection, reconnect, and revocation without introducing a scheduler or live migration system.

## ADDED Requirements

### Requirement: Active sessions have one durable daemon owner

The server SHALL select a connected, eligible daemon when a session is created or
started, considering repository/runtime capabilities when needed. An active
session SHALL persist a non-null `daemon_id`, and every command/event for that
session SHALL be accepted only from that daemon identity. A session SHALL NOT
float between daemons. A session created before start MAY be an unassigned
record, but the server SHALL assign and persist its owner atomically before
creating the first command.

#### Scenario: Session pins the selected daemon

- **WHEN** the server starts a session against daemon D1
- **THEN** the session stores `daemon_id = D1` and commands for that session are routed only to D1

#### Scenario: Another daemon cannot claim a session

- **WHEN** daemon D2 reconnects and presents a valid connection while a session is pinned to D1
- **THEN** the server refuses D2's session frames and performs no session state change

### Requirement: Reconnect preserves ownership and never migrates live work

A reconnect from the same daemon identity SHALL resume only sessions pinned to
that identity. North 0.1.0 SHALL perform no automatic live migration. If the
owner is unavailable, server retry/failure policy applies to the pinned
session; a different daemon is not silently selected. A new daemon may be
selected only for a new session or an explicit future migration feature.

#### Scenario: Owner reconnect resumes its session

- **WHEN** D1 disconnects and later reconnects with an unrevoked credential
- **THEN** D1 may reconcile and resume its pinned sessions, preserving command/event sequence state

#### Scenario: Offline owner does not reassign work

- **WHEN** D1 is unavailable during an active session
- **THEN** the session enters server-owned retry handling or terminal failure and remains owned by D1

### Requirement: Daemon credentials are user-owned instance registrations

A daemon registration is an instance-scoped identity, while its credential is
owned by the North account named by `created_by`; `created_by` means the account
that authorized setup and owns the registration, not the actor that later
executes a session. Credentials are not transferable or shared across users in
0.1.0. The credential owner MAY revoke its own credential, and Admin/Owner MAY
revoke any credential. Revocation SHALL close current access and refuse future
connections; pinned sessions are not reassigned and follow normal retry or
failure handling.

#### Scenario: Revocation cuts current and future access

- **WHEN** an Admin revokes a daemon credential
- **THEN** the live connection is closed, subsequent handshakes fail, and its pinned sessions remain pinned rather than migrating

#### Scenario: Credential ownership is auditable

- **WHEN** a daemon registration is inspected
- **THEN** its `created_by` identifies the account that created it and no other user can silently claim or share it
