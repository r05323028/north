# conversations Specification

## Purpose

Provides dialogue around a requirement while preserving structured Requirement
state as canonical and separating content revision from mutable-state concurrency.

## Requirements

### Requirement: One conversation per requirement

Each requirement SHALL have exactly one conversation thread; requester and
agent messages append chronologically and paginate deterministically. The
conversation SHALL remain supporting context and SHALL NOT be used as
canonical Requirement state.

#### Scenario: History survives reloads

- **WHEN** a user reopens a requirement after days
- **THEN** prior messages appear in order with authors and timestamps while structured fields come from the Requirement endpoint

### Requirement: Message kinds exclude raw telemetry

Messages SHALL be kind requester, agent, or system. Raw tool output and model
chain-of-thought SHALL NOT be stored as or converted into messages.

#### Scenario: Tool noise stays out of chat

- **WHEN** the runtime reports internal activity
- **THEN** it never appears appended to the conversation as a message

### Requirement: Structured edits ride the domain contract

Editing structured requirement fields through the conversation surface SHALL
require `expected_state_version` and apply the same domain rules as direct
edits: one `revision` and one `state_version` increment per accepted content
edit, Ready demotion on a real edit, refusal in terminal states, and an atomic
state-version check. `revision` remains content concurrency/readiness identity;
`state_version` is mutable-state concurrency. Empty `summary` and empty list
fields are valid intentional values, while title, description, and individual
list entries remain trimmed and non-empty.

#### Scenario: Edit from conversation view bumps versions

- **WHEN** a requester updates assumptions from the detail pane with the current expected_state_version
- **THEN** the returned revision and state_version are each previous+1 and any Ready state is demoted

#### Scenario: Stale conversation edit is conflict-safe

- **WHEN** the detail view submits an edit for an older state_version
- **THEN** HTTP 409 is returned without changing structured state, audit history, or appending a message

#### Scenario: Summary can be cleared

- **WHEN** the detail view submits `summary: ""` with the current expected_state_version
- **THEN** the persisted summary is empty and the content edit increments both tokens once

#### Scenario: No-op conversation edit is inert

- **WHEN** the detail view submits unchanged fields or empty-allowed optional fields without a content change
- **THEN** neither revision nor state_version changes and Ready is not demoted

### Requirement: Structured state is readable without replay

Current structured Requirement state SHALL be retrievable directly, including
both `revision` and `state_version`; consumers SHALL NOT need to read or replay
messages to know the specification. Archiving or pruning messages SHALL NOT
alter any structured field.

#### Scenario: Empty inbox, intact spec

- **WHEN** all messages are removed in a test fixture
- **THEN** the requirement's fields, status, revision, and state_version are unchanged and fully served by the structured endpoint

### Requirement: Conversation access follows workspace collaboration policy

Authenticated users SHALL be able to read and append conversation context for
any Requirement in their North instance. Requesters SHALL be able to edit
non-terminal structured fields through the conversation surface; review-only
transitions remain role-gated and every mutation remains state-version guarded.
No per-Requirement ACL SHALL be inferred from conversation authorship.

#### Scenario: Requester uses another user's conversation

- **WHEN** a Requester reads or appends a message to a known Requirement conversation
- **THEN** the operation succeeds without an ownership check and raw telemetry is not accepted as a message kind
