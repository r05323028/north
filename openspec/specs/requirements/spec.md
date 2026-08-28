# requirements Specification

## Purpose

Owns the requirement as a durable, revision-tracked object whose lifecycle can
only move along legal, permission-checked edges regardless of caller.

## Requirements

### Requirement: Creation produces a Draft at revision 1

Creating a requirement with title and description SHALL produce a Draft
requirement at revision 1 attributed to its creator.

#### Scenario: New requirement starts clean

- **WHEN** a requester submits title + description
- **THEN** the persisted requirement has status Draft, revision 1, and
created_by of the caller

### Requirement: Only legal transitions are representable

Every status change SHALL pass through the domain transition table
(Draft→Discussing; Ready→Accepted; Ready→Rejected; Ready→Discussing as
Request Changes; Rejected→Discussing as Reopen; plus the server-validated
Discussing→Ready edge). Illegal or stale requests SHALL fail without side
effects.

#### Scenario: Accept requires Ready

- **WHEN** anyone attempts to accept a Discussing requirement
- **THEN** the request fails and status remains Discussing

#### Scenario: Request Changes differs from Reject

- **WHEN** a reviewer requests changes on a Ready requirement with feedback
- **THEN** status becomes Discussing with reviewer feedback preserved, and the
requirement is NOT Rejected

#### Scenario: Valid assessment reaches Ready

- **WHEN** a current-revision assessment has a Ready verdict, no blockers, and meaningful acceptance criteria
- **THEN** the server persists its typed evidence and moves the Discussing requirement to Ready atomically

#### Scenario: Invalid assessment cannot reach Ready

- **WHEN** an assessment is stale, blocked, lacks criteria, or requests clarification
- **THEN** the server durably records rejection and leaves Requirement state unchanged

### Requirement: Human review transitions are reviewer-gated

Accept, Reject, Request Changes, and Reopen SHALL require the reviewer role
(Requirement Manager/Admin/Owner) enforced server-side.

#### Scenario: Requester cannot accept

- **WHEN** a Requester submits Accept on a Ready requirement
- **THEN** the request fails on permissions before any state check

### Requirement: Edits bump revision and respect terminality

Every accepted content edit SHALL increment revision by exactly one. Editing
a Ready requirement SHALL demote it to Discussing (stale-assessment
demotion). Accepted and Rejected requirements SHALL refuse direct edits.

#### Scenario: Edit while Ready forces re-clarification

- **WHEN** a requester edits criteria on a Ready requirement
- **THEN** revision increments once and status drops to Discussing

#### Scenario: Terminal silence

- **WHEN** anyone edits an Accepted requirement
- **THEN** the request fails and nothing changes

### Requirement: Queryable list with deterministic ordering

The system SHALL expose listing with search over text fields, filtering by
status and creator, and sorting by updated time — sufficient for board and
list views without client-side full scans.

#### Scenario: Board feeds itself from the API

- **WHEN** a client requests requirements grouped by status
- **THEN** results are complete and ordered deterministically per sort key

### Requirement: Existing Requirement mutations are revision-aware

Every mutation of an existing Requirement SHALL require `expected_revision`,
including structured edits submitted from the conversation surface. Persistence
SHALL compare it atomically with the current row before applying a domain
operation. A stale value SHALL return a conflict (normally HTTP 409) and SHALL
persist no content, status, revision, audit, message, or other side effect.

#### Scenario: Stale caller cannot overwrite newer state

- **WHEN** a client loaded revision 12, another actor committed revision 13, and the client submits `expected_revision = 12`
- **THEN** the mutation returns HTTP 409 and the revision-13 row remains unchanged

#### Scenario: Conversation edit uses the domain contract

- **WHEN** a requester saves structured fields from a conversation detail view
- **THEN** the server applies the same content-edit rules as the direct requirement endpoint

#### Scenario: Stale conversation edit cannot append context

- **WHEN** a conversation edit targets an older revision
- **THEN** the mutation returns HTTP 409 and leaves both structured state and conversation history unchanged
