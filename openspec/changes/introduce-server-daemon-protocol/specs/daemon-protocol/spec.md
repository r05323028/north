## Purpose

Defines the single wire contract between server and daemon: explicit typed
messages, stable identifiers, at-least-once delivery with idempotent
processing, and reconnect/resume that needs no ceremony.

## ADDED Requirements

### Requirement: Explicit envelope with stable identifiers and disjoint directions

Every message SHALL carry a stable unique id, a session id, a type tag, a
payload, and a schema version. Messages belong to one of three disjoint
groups whose direction is part of their identity: connection/control frames
(daemon→server hello/registration and heartbeat; server→daemon acknowledgement
of durably processed event ids; server→daemon resume/reconciliation state),
server commands (session.start, session.cancel, session.resume, message.send —
server→daemon ONLY), and daemon events (session.started, agent.message,
agent.activity, requirement.assessed, session.completed, session.failed —
daemon→server ONLY). Command/event families SHALL be exhaustive enums in the
shared wire crate, which SHALL NOT depend on business crates. No message name
appears in both directions: `session.resume` is never an event.

#### Scenario: Wire crate purity

- **WHEN** architecture tests inspect north-protocol dependencies
- **THEN** no business crate edge exists

### Requirement: At-least-once delivery with harmless duplicates

Senders MAY retry any message with its original id after reconnect. Receivers
SHALL process each unique id at most once for durable effects; duplicate
delivery SHALL produce no duplicated state change.

#### Scenario: Retry storm is safe

- **WHEN** the same requirement.assessed event arrives three times
- **THEN** exactly one promotion occurs and replies acknowledge all copies

### Requirement: Resume across reconnects with explicit acknowledgement

On reconnect the daemon SHALL be able to resume sessions and re-deliver
buffered unacknowledged events in order. The server SHALL explicitly
acknowledge durably processed event ids via a dedicated control frame; only
acknowledged ids may leave the daemon replay buffer. Duplicates remain inert
under at-least-once delivery.

#### Scenario: Drop mid-session recovers

- **WHEN** connectivity drops while events are in flight and returns later
- **THEN** buffered events arrive once each and the session continues without
manual intervention
