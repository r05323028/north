## Purpose

Defines the single wire contract between server and daemon: explicit typed
messages, stable identifiers, at-least-once delivery with idempotent
processing, and reconnect/resume that needs no ceremony.

## ADDED Requirements

### Requirement: Explicit envelope with stable identifiers

Every message SHALL carry a stable unique id, a session id, a type tag, a
payload, and a schema version. Command families (session.start,
session.cancel, session.resume, message.send) and event families
(session.started, agent.message, agent.activity, requirement.assessed,
session.completed, session.failed) SHALL be exhaustive enums in the shared
wire crate, which SHALL NOT depend on business crates.

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

### Requirement: Resume across reconnects

On reconnect the daemon SHALL be able to resume sessions and re-deliver
buffered unacknowledged events in order; the server SHALL acknowledge
processing so the daemon can trim its buffer.

#### Scenario: Drop mid-session recovers

- **WHEN** connectivity drops while events are in flight and returns later
- **THEN** buffered events arrive once each and the session continues without
manual intervention
