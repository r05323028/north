## Purpose

Keeps browser live updates lightweight and honest: SSE nudges the UI to refetch canonical server state instead of becoming a second durable event-sourcing system.

## ADDED Requirements

### Requirement: SSE is notification, not canonical state

North browser live updates SHALL use HTTP and SSE only. An SSE payload MAY
include a lightweight event id/cursor and notification kind, but it SHALL NOT
be required to contain enough history to reconstruct Requirement truth. The
canonical Requirement, conversation, assessment, and execution state SHALL be
read from server HTTP APIs/database-backed responses.

#### Scenario: Notification prompts canonical refresh

- **WHEN** another actor changes a visible Requirement and the browser receives an SSE notification
- **THEN** the browser refetches the affected canonical API state rather than applying an inferred lifecycle transition from the notification

#### Scenario: Stream loss does not erase truth

- **WHEN** the SSE connection disconnects
- **THEN** the UI retains last-known data, reconnects using EventSource/HTTP, and can recover current truth by refetching the server API

### Requirement: Reconnect never depends on SSE replay

After an SSE disconnect or page reload, the browser SHALL be able to refetch
canonical state without replaying a durable SSE stream. `Last-Event-ID` and
lightweight cursors MAY suppress redundant hints, but missed or duplicated SSE
notifications SHALL be harmless. The frontend SHALL never open a WebSocket.
Conversation/history remains context; structured Requirement state remains
canonical truth.

#### Scenario: Missed notification is repaired by refetch

- **WHEN** a status-change notification is missed while the browser is offline
- **THEN** reconnect/refetch returns the current server state, even if no SSE event is replayed

#### Scenario: Duplicate hint has no business effect

- **WHEN** the same SSE notification arrives twice
- **THEN** the UI performs at most a harmless refetch and never duplicates a message, transition, or Requirement mutation
