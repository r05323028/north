## Purpose

Provides the dialogue around a requirement while guaranteeing the structured
requirement stays the single source of truth.

## ADDED Requirements

### Requirement: One conversation per requirement

Each requirement SHALL have exactly one conversation thread; requester and
agent messages append chronologically and paginate deterministically.

#### Scenario: History survives reloads

- **WHEN** a user reopens a requirement after days
- **THEN** prior messages appear in order with authors and timestamps

### Requirement: Message kinds exclude raw telemetry

Messages SHALL be kind requester, agent, or system. Raw tool output and model
chain-of-thought SHALL NOT be stored as or converted into messages.

#### Scenario: Tool noise stays out of chat

- **WHEN** the runtime reports internal activity
- **THEN** it never appears appended to the conversation as a message

### Requirement: Structured edits ride the domain contract

Editing structured requirement fields through the conversation surface SHALL
apply the same domain rules as direct edits: one revision increment per
accepted edit, Ready demotion on edit, refusal in terminal states.

#### Scenario: Edit from conversation view bumps revision

- **WHEN** a requester updates assumptions from the detail pane
- **THEN** the returned revision is previous+1 and any Ready state demoted

### Requirement: Structured state is readable without replay

Current structured requirement state SHALL be retrievable directly; consumers
SHALL NOT need to read or replay messages to know the specification.
Archiving or pruning messages SHALL NOT alter any structured field.

#### Scenario: Empty inbox, intact spec

- **WHEN** all messages are removed in a test fixture
- **THEN** the requirement's fields, status, and revision are unchanged and
fully served by the structured endpoint

### Requirement: Structured edits reject stale revisions

The structured-edit endpoint SHALL require `expected_revision` and use the
same atomic revision check as direct Requirement edits. A stale edit SHALL
return HTTP 409 and SHALL NOT append a message, bump revision, demote status,
or write an audit row.

#### Scenario: Conversation edit loses a revision race

- **WHEN** the detail view sends an edit for revision 12 after another actor committed revision 13
- **THEN** the endpoint returns HTTP 409 and structured state plus conversation history remain unchanged
