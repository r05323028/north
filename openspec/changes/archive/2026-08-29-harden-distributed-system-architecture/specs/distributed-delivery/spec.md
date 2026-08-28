## Purpose

Defines reconnect-safe server-to-daemon command delivery and daemon-to-server event reconciliation without introducing a message broker or changing North's transport topology.

## ADDED Requirements

### Requirement: Commands cross a durable idempotency boundary

Every server command SHALL carry a stable `command_id`, `session_id`, and
monotonic `server_command_seq`. The server SHALL durably persist a command
before dispatching it and SHALL retain it until a daemon `command_ack`
acknowledges durable receipt. Delivery SHALL be at least once. The daemon SHALL
persist an inbox/processed-command record keyed by daemon identity and
`command_id` before sending `command_ack`.

`command_ack` means the command is durably recorded for processing; it
MUST NOT be interpreted as runtime completion. Runtime completion and failure
remain facts/events handled by server execution policy. A duplicate command
MUST return the same acceptance outcome and MUST NOT invoke the runtime again
or duplicate a side effect. In particular, one `message.send` command SHALL
submit its user message to the agent at most once.

#### Scenario: Reconnect retries one message safely

- **WHEN** a `message.send` frame is lost after server outbox persistence and the server resends the same `command_id`
- **THEN** the daemon durably accepts the id once, submits the message at most once, and acknowledges duplicate delivery without another runtime submission

#### Scenario: Accepted command survives daemon restart

- **WHEN** the daemon commits an inbox record and crashes before runtime completion
- **THEN** restart recovers that record without issuing a second invocation; it reattaches to the idempotent operation keyed by `command_id` or reports an explicit unknown outcome for server retry policy

### Requirement: Processed-command history has a safe compaction boundary

The daemon SHALL NOT remove a processed-command identity while its session is
active or while the server can resend the command. After a session reaches a
terminal execution state and reconciliation confirms a contiguous command
watermark, the daemon MAY replace individual records through that watermark
with a durable per-session `processed_through_seq` tombstone. Sparse records
above the watermark SHALL remain. The watermark SHALL be retained for the
life of the durable session, so a late duplicate at or below it remains a
no-op. North 0.1.0 SHALL NOT expire this tombstone by time alone.

#### Scenario: Terminal session compacts without reopening a side effect

- **WHEN** all commands through sequence 8 are terminal, acknowledged, and the session is terminal
- **THEN** individual records through 8 may be compacted to a durable high-water mark and a late sequence-7 duplicate is rejected or acknowledged as already processed without runtime invocation

### Requirement: Directional sequence spaces reconcile gaps

For each session, server command sequences and daemon event sequences SHALL be
independent, monotonic, and persisted before transmission. The daemon SHALL
replay buffered events in `daemon_event_seq` order. The server SHALL process
events in contiguous order, buffer or request replay for a gap, and commit
business effects before acknowledging the event sequence. Each
`SessionReconcileState` in the connection-level snapshot SHALL carry command and
event contiguous watermarks and MAY carry a canonical `event_ack_sparse` list
when processing is non-contiguous. When present, the list SHALL contain only
values above `event_ack_through_seq`, contain no duplicates, and be strictly
ascending.

An id identifies one delivery for idempotency; a sequence orders deliveries
and detects gaps. A duplicate with the same id and sequence SHALL be harmless
and re-acknowledged. The same sequence with a different id SHALL be a protocol
error. A late frame at or below an acknowledged contiguous sequence SHALL have
no business effect. An out-of-order frame MAY be durably buffered but SHALL
NOT be applied to business state until its gap is reconciled.

#### Scenario: Event gap blocks premature promotion

- **WHEN** daemon event sequence 4 arrives while sequence 3 is missing
- **THEN** the server records no effect from sequence 4, requests/reconciles sequence 3, and applies events deterministically in sequence order

#### Scenario: Duplicate and late frames stay inert

- **WHEN** an already committed event or command is delivered again, including after reconnect
- **THEN** North returns the applicable acknowledgement and performs no second durable effect or runtime invocation

### Requirement: Reconciliation is a connection-level snapshot

The server SHALL send one finite `ReconcileSnapshot` per authenticated daemon
connection. The snapshot MAY contain zero sessions or SHALL contain one unique
`SessionReconcileState` for each session pinned to that daemon. Each state SHALL
carry command/event contiguous ACK watermarks and a canonical `event_ack_sparse`
list. The wire contract SHALL reject empty session IDs, sparse values at or below
`event_ack_through_seq`, duplicate sparse values, non-ascending sparse values,
and duplicate session IDs.

#### Scenario: Empty reconciliation is explicit

- **WHEN** a daemon has no sessions pinned to it
- **THEN** the server sends one snapshot with an empty session list

#### Scenario: One connection reconciles multiple sessions

- **WHEN** a daemon has multiple pinned sessions
- **THEN** one snapshot carries distinct state for every pinned session

### Requirement: Compatible peers and unknown frames fail deterministically

Daemon hello/registration and the server welcome SHALL carry exact
`protocol_version` `0.1`. Each frame SHALL carry `schema_version` `1` for the
0.1.0 catalog. A peer with no exact protocol-version match SHALL receive a
`protocol.error` with an incompatibility code and the connection SHALL close
before session traffic. An unknown command/event type or unsupported schema
version on an otherwise compatible connection SHALL receive an explicit
`protocol.error`, SHALL have no side effect, and SHALL close that connection. A
`protocol.error` has no severity discriminator and is terminal to the current
connection; the host decides whether a future connection may be attempted.
Unacknowledged durable messages remain eligible for reconciliation after an
upgrade. North 0.1.x SHALL not negotiate plugin ranges or silently reinterpret
unknown payloads.

#### Scenario: Incompatible daemon cannot start work

- **WHEN** a daemon registers protocol version `0.2` with a 0.1 server
- **THEN** the server sends an incompatibility protocol error, refuses registration, and starts no session

#### Scenario: Unknown frame is never guessed

- **WHEN** a compatible peer sends an unknown command or unsupported schema version
- **THEN** the receiver sends a protocol error, performs no side effect, closes the connection, and retains any unacknowledged durable message for later reconciliation
