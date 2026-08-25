## Purpose

Runs the clarify-assess loop: agent sessions grounded in structured requirements and isolated local repositories, ending in a validated readiness verdict while the server owns all business effects.

## ADDED Requirements

### Requirement: Sessions carry complete business context and one owner

Starting a session SHALL provide the agent the current structured requirement,
relevant conversation context, and the enabled configured repository catalog
(metadata only). The server SHALL persist the selected `daemon_id` before
`session.start`; the agent SHALL NOT receive database access or business write
paths.

#### Scenario: Agent sees the spec, not the schema

- **WHEN** `session.start` is dispatched
- **THEN** its context contains requirement fields, thread excerpt, enabled repository metadata, and the pinned session identity without persistence access

### Requirement: Output arrives as typed protocol events

Agent dialogue SHALL surface as `agent.message` events (persisted as
conversation messages); progress SHALL surface as coarse `agent.activity`
events. Events carry their stable id and `daemon_event_seq`. Raw tool dumps and
model chain-of-thought SHALL NOT be forwarded as messages or activity payloads.

#### Scenario: Activity stays high-level

- **WHEN** the runtime performs many internal tool calls
- **THEN** clients observe summarized activity entries, never raw logs

### Requirement: Repository inspection rides an isolated local runtime

When repository context matters, the runtime SHALL inspect through the daemon's
host-Git capability using a unique session/task disposable checkout and include
repository identities plus exact SHAs in any produced assessment. A dirty
checkout SHALL be discarded and reported.

#### Scenario: Concurrent sessions do not contaminate each other

- **WHEN** two sessions inspect the same configured repository concurrently
- **THEN** each uses a different disposable checkout and neither runtime can use the other's mutable files or cache

### Requirement: Server owns all business transitions and assessment ACKs

Sessions SHALL NOT mutate Requirement state directly. Every effect flows through
server-side expected-revision validation, event dedupe, domain gates, immutable
evidence persistence, and the single assessment transaction. The server sends
the event ACK only after commit; a stale/invalid event receives durable
rejection handling and cannot promote Ready.

#### Scenario: Daemon cannot force Ready

- **WHEN** a crafted `requirement.assessed` event violates revision binding
- **THEN** the server commits a rejection/dedupe record, acknowledges that rejection, and leaves Requirement state unchanged

### Requirement: Cancellation is idempotent and owner-routed

Canceling a session SHALL send one durable `session.cancel` command to its
pinned daemon. Repeated delivery of that command SHALL stop the runtime at
most once and emit completion/failure facts without corrupting accepted
outputs. A command retry or reconnect SHALL NOT append duplicate agent
messages.

#### Scenario: User cancels mid-run

- **WHEN** the requester cancels an active session and the connection retries the cancel command
- **THEN** the pinned daemon halts the runtime once, prior messages remain intact, and no duplicate cancellation side effect occurs
