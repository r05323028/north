# distributed-delivery Specification

## Purpose

Defines reconnect-safe server-to-daemon command delivery and daemon-to-server event reconciliation without introducing a message broker or changing North's transport topology.

## Requirements

### Requirement: Commands cross a durable idempotency boundary

Every server command SHALL carry stable `command_id`, `session_id`, and monotonic `server_command_seq`. The server SHALL persist it before at-least-once dispatch and retain it until durable daemon `command_ack`. The daemon SHALL persist an inbox identity keyed by daemon identity and `command_id` before ACK. ACK means durable receipt, not runtime completion. The coordinator SHALL gate dispatch with idempotency; duplicates SHALL not repeat side effects or submit `message.send` twice.

#### Scenario: Reconnect retries one message safely

- **WHEN** a `message.send` frame is lost after server outbox persistence and the server resends the same `command_id`
- **THEN** the daemon durably accepts the id once, submits the message at most once, and acknowledges duplicate delivery without another runtime submission

#### Scenario: Duplicate delivery preserves acceptance

- **WHEN** the daemon receives a command ID again after durable acceptance
- **THEN** it returns the same acceptance outcome without another side effect or runtime invocation; one `message.send` is submitted at most once

#### Scenario: Command acknowledgement is not runtime completion

- **WHEN** a `session.start` dispatch is acknowledged while its runtime continues
- **THEN** `command_ack` means durable recording only; the command journal records `dispatch_succeeded`, `dispatch_failed`, or `unknown` (`completed`/`failed` may be aliases) for that command, while the session remains Running until server execution policy handles later `session.completed` or `session.failed` facts/events

#### Scenario: Durable coordinator owns the dispatch seam

- **WHEN** the durable command coordinator prepares runtime dispatch
- **THEN** it decides journal state and idempotency before a narrow internal seam accepting stable command and runtime-operation identity; tests MAY use a deterministic fake executor and `clarification-runtime` provides the real adapter. This contract does not define agent prompting, SDK behavior, tool choice, repository inspection, or readiness judgment.

#### Scenario: Accepted command survives daemon restart

- **WHEN** the daemon commits an inbox record and crashes before runtime completion
- **THEN** restart recovers that record without issuing a second invocation; it reattaches to the idempotent operation keyed by `command_id` or reports an explicit unknown outcome for server retry policy

### Requirement: Processed-command history has a safe compaction boundary

Daemon SHALL retain processed-command identities while a session is active or the server can resend. After terminal execution and reconciliation confirms a contiguous command watermark, it MAY replace records through it with a durable per-session `processed_through_seq`; sparse records above remain. Keep the watermark for the durable-session lifetime; North 0.1.0 SHALL NOT expire it by time alone.

#### Scenario: Terminal session compacts without reopening a side effect

- **WHEN** all commands through sequence 8 are terminal, acknowledged, and the session is terminal
- **THEN** individual records through 8 may be compacted to a durable high-water mark and a late sequence-7 duplicate is rejected or acknowledged as already processed without runtime invocation

### Requirement: Directional sequence spaces reconcile gaps

Server-command and daemon-event sequences SHALL be independent, monotonic, persisted before send; daemon SHALL replay in order. Server SHALL process events contiguously, buffer/replay gaps, and commit effects before ACK. Connection snapshots carry both watermarks and MAY include sparse ACKs. Duplicate ID+sequence is harmless and re-ACKed; the same sequence with another ID is a protocol error; late ACKed frames are inert; out-of-order events SHALL wait for gap closure before effects.

#### Scenario: Sparse event acknowledgements are canonical

- **WHEN** a connection snapshot includes non-contiguous event acknowledgements for a session
- **THEN** each `SessionReconcileState` carries command and event contiguous watermarks; optional `event_ack_sparse` contains only unique ascending values above `event_ack_through_seq`

#### Scenario: Event gap blocks premature promotion

- **WHEN** daemon event sequence 4 arrives while sequence 3 is missing
- **THEN** the server records no effect from sequence 4, requests/reconciles sequence 3, and applies events deterministically in sequence order

#### Scenario: Duplicate and late frames stay inert

- **WHEN** an already committed event or command is delivered again, including after reconnect
- **THEN** North returns the applicable acknowledgement and performs no second durable effect or runtime invocation

### Requirement: Reconciliation is a connection-level snapshot

Server SHALL send one finite `ReconcileSnapshot` per authenticated daemon connection: zero sessions or one unique `SessionReconcileState` per pinned session. Each state carries command/event ACK watermarks and canonical `event_ack_sparse`. Wire validation SHALL reject empty/duplicate session IDs, duplicate or non-ascending sparse values, and sparse values at or below `event_ack_through_seq`.

#### Scenario: Empty reconciliation is explicit

- **WHEN** a daemon has no sessions pinned to it
- **THEN** the server sends one snapshot with an empty session list

#### Scenario: One connection reconciles multiple sessions

- **WHEN** a daemon has multiple pinned sessions
- **THEN** one snapshot carries distinct state for every pinned session

### Requirement: Compatible peers and unknown frames fail deterministically

Daemon hello and server welcome SHALL use exact `protocol_version` `0.1`; each frame SHALL use `schema_version` `1`. Version mismatch SHALL send incompatibility `protocol.error` and close before session traffic. Unknown command/event types or unsupported schema SHALL send explicit error, cause no effect, and close. Unacknowledged durable messages remain reconcilable after upgrades; North 0.1.x SHALL NOT negotiate plugin ranges or reinterpret unknown payloads.

#### Scenario: Incompatible daemon cannot start work

- **WHEN** a daemon registers protocol version `0.2` with a 0.1 server
- **THEN** the server sends an incompatibility protocol error, refuses registration, and starts no session

#### Scenario: Protocol error terminates one connection

- **WHEN** a peer receives `protocol.error`
- **THEN** the error has no severity discriminator and ends the current connection; the host determines whether a future connection may be attempted

#### Scenario: Unknown frame is never guessed

- **WHEN** a compatible peer sends an unknown command or unsupported schema version
- **THEN** the receiver sends a protocol error, performs no side effect, closes the connection, and retains any unacknowledged durable message for later reconciliation
