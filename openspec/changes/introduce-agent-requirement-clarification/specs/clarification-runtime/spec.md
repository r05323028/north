## Purpose

Runs the clarify-assess loop: agent sessions grounded in structured
requirements and real repositories, streaming dialogue and activity, ending in
a validated readiness verdict — with the server owning all business effects.

## ADDED Requirements

### Requirement: Sessions carry complete business context

Starting a session SHALL provide the agent the current structured requirement,
relevant conversation context, and the configured repository catalog (metadata
only). The agent SHALL NOT receive database access or business write paths.

#### Scenario: Agent sees the spec, not the schema

- **WHEN** session.start is dispatched
- **THEN** its context contains requirement fields, thread excerpt, and repo
metadata — and nothing resembling persistence access

### Requirement: Output arrives as typed protocol events

Agent dialogue SHALL surface as agent.message events (persisted as
conversation messages); progress SHALL surface as coarse agent.activity events.
Raw tool dumps and model chain-of-thought SHALL NOT be forwarded as messages
or activity payloads.

#### Scenario: Activity stays high-level

- **WHEN** the runtime performs many internal tool calls
- **THEN** clients observe summarized activity entries, never raw logs

### Requirement: Repository inspection rides the local runtime

When repository context matters, the runtime SHALL inspect via the daemon's
local-git capability and include inspected identities+SHAs in any produced
assessment.

#### Scenario: Assessment cites inspected code

- **WHEN** a session consults a repository before assessing
- **THEN** the emitted requirement.assessed lists that repository with the
exact SHA

### Requirement: Server owns all business transitions

Sessions SHALL NOT mutate requirement state directly; every effect flows
through server-side validation (assessment gates, transition table). A
completed session without a valid assessment SHALL leave Ready unreachable.

#### Scenario: Daemon cannot force Ready

- **WHEN** a crafted requirement.assessed event violates revision binding
- **THEN** the server refuses promotion exactly as with any other claimant

### Requirement: Cancellation stops work promptly

canceling a session SHALL stop the runtime at the next opportunity and emit
session.completed or session.failed accordingly, without corrupting partial
outputs already accepted.

#### Scenario: User cancels mid-run

- **WHEN** the requester cancels an active session
- **THEN** the runtime halts, prior messages remain intact, and no further
agent messages append afterward
