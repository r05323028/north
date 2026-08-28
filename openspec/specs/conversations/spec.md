# conversations Specification

## Purpose

Provides the dialogue around a requirement while guaranteeing the structured
requirement stays the single source of truth.

## Requirements

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
accepted edit, Ready demotion on edit, refusal in terminal states, and an
atomic `expected_revision` check.

#### Scenario: Edit from conversation view bumps revision

- **WHEN** a requester updates assumptions from the detail pane
- **THEN** the returned revision is previous+1 and any Ready state demoted

#### Scenario: Stale conversation edit is conflict-safe

- **WHEN** the detail view submits an edit for an older revision
- **THEN** HTTP 409 is returned without changing structured state or appending a message

### Requirement: Structured state is readable without replay

Current structured requirement state SHALL be retrievable directly; consumers
SHALL NOT need to read or replay messages to know the specification.
Archiving or pruning messages SHALL NOT alter any structured field.

#### Scenario: Empty inbox, intact spec

- **WHEN** all messages are removed in a test fixture
- **THEN** the requirement's fields, status, and revision are unchanged and
fully served by the structured endpoint
