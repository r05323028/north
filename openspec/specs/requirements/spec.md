# requirements Specification

## Purpose

Owns the requirement as a durable object whose canonical content revision and
mutable state version support legal, permission-checked lifecycle operations.

## Requirements

### Requirement: Creation produces a Draft at revision 1

Creating a requirement with title and description SHALL produce a Draft
requirement at revision 1 and state_version 1 attributed to its creator.
`revision` identifies canonical structured content; `state_version` identifies
mutable Requirement state.

#### Scenario: New requirement starts clean

- **WHEN** a requester submits title + description
- **THEN** the persisted requirement has status Draft, revision 1, state_version 1, and created_by of the caller

### Requirement: Only legal transitions are representable

Every status change SHALL pass through the domain transition table
(Draft→Discussing; Ready→Accepted; Ready→Rejected; Ready→Discussing as
Request Changes; Rejected→Discussing as Reopen; plus the server-validated
Discussing→Ready edge). A successful persisted status mutation SHALL increment
`state_version` exactly once without changing `revision`. Illegal, stale,
assessment-rejected, or duplicate requests SHALL fail or be idempotent without
changing Requirement state or its version tokens.

#### Scenario: Accept requires Ready

- **WHEN** anyone attempts to accept a Discussing requirement
- **THEN** the request fails and status, revision, and state_version remain unchanged

#### Scenario: Request Changes differs from Reject

- **WHEN** a reviewer requests changes on a Ready requirement with feedback
- **THEN** status becomes Discussing, state_version increments once, reviewer feedback is preserved, and the requirement is NOT Rejected

#### Scenario: Valid assessment reaches Ready

- **WHEN** a current-revision assessment has a Ready verdict, no blockers, and meaningful acceptance criteria
- **THEN** the server persists its typed evidence and moves the Discussing requirement to Ready atomically while incrementing state_version once

#### Scenario: Invalid assessment cannot reach Ready

- **WHEN** an assessment is stale, blocked, lacks criteria, or requests clarification
- **THEN** the server durably records rejection and leaves Requirement status, revision, and state_version unchanged

### Requirement: Human review transitions are reviewer-gated

Accept, Reject, Request Changes, and Reopen SHALL require the reviewer role
(Requirement Manager/Admin/Owner) enforced server-side. Accept, Reject, and
Request Changes SHALL also require `expected_state_version` and the
`assessment_id` from the reviewed packet. Persistence SHALL atomically verify
the expected state version, current revision, Ready state, and that assessment
is the currently valid accepted readiness assessment for that revision and
Ready state generation. Reopen SHALL require the expected state version but
not an assessment identity.

#### Scenario: Requester cannot accept

- **WHEN** a Requester submits Accept on a Ready requirement
- **THEN** the request fails on permissions before any state or audit mutation

#### Scenario: Stale reviewer cannot decide

- **WHEN** a reviewer submits a packet's old state version or assessment_id after Request Changes and a new Ready assessment
- **THEN** HTTP 409 is returned and status, state_version, and audit history remain unchanged

#### Scenario: Current reviewer decision commits once

- **WHEN** an authorized reviewer submits the current assessment_id and expected_state_version
- **THEN** the legal transition and one audit row commit atomically and state_version increments exactly once

### Requirement: Edits bump revision and respect terminality

Every accepted content edit SHALL increment `revision` and `state_version` by
exactly one. Editing a Ready requirement SHALL demote it to Discussing while
incrementing both values once. Accepted and Rejected requirements SHALL refuse
direct edits. A no-op edit SHALL increment neither value and SHALL NOT demote
Ready. Optional `summary` and list fields may intentionally be empty; title,
description, and list entries retain their non-empty validation rules.

#### Scenario: Edit while Ready forces re-clarification

- **WHEN** a requester edits criteria on a Ready requirement
- **THEN** revision and state_version each increment once and status drops to Discussing

#### Scenario: No-op does not mutate state

- **WHEN** a client submits only unchanged or empty-allowed optional values
- **THEN** status, revision, state_version, audit history, and conversation history remain unchanged

#### Scenario: Terminal silence

- **WHEN** anyone edits an Accepted requirement
- **THEN** the request fails and nothing changes

#### Scenario: Optional summary can be cleared

- **WHEN** a client submits `summary: ""` with the current expected_state_version
- **THEN** the persisted summary is empty and the real edit increments revision and state_version once

### Requirement: Queryable list with deterministic ordering

The system SHALL expose listing with search over text fields, filtering by
status and creator, and sorting by updated time — sufficient for board and list
views without client-side full scans.

#### Scenario: Board feeds itself from the API

- **WHEN** a client requests requirements grouped by status
- **THEN** results are complete and ordered deterministically per sort key

### Requirement: Existing Requirement mutations are revision-aware

Every user-driven mutation of an existing Requirement SHALL require
`expected_state_version`. Persistence SHALL compare it atomically with the
current row before applying a domain operation. Server-owned readiness
ingestion instead locks the current row and matches assessment evidence on
`requirement_revision`; it increments `state_version` only for a successful
promotion. `revision` remains the canonical structured-content revision and
`state_version` changes on every real persisted Requirement mutation. A stale
state version SHALL return a conflict (normally HTTP 409) and SHALL persist no
content, status, revision, state_version, audit, message, or other side effect.
No-op edits increment neither token.

#### Scenario: Stale caller cannot overwrite newer state

- **WHEN** a client loaded state_version 12, another actor committed a lifecycle mutation, and the client submits expected_state_version = 12
- **THEN** the mutation returns HTTP 409 and the newer row remains unchanged even if revision is unchanged

#### Scenario: Conversation edit uses state concurrency

- **WHEN** a requester saves structured fields from a conversation detail view
- **THEN** the server requires expected_state_version and applies the same content-edit rules as the direct requirement endpoint

#### Scenario: Stale conversation edit cannot append context

- **WHEN** a conversation edit targets an older state_version
- **THEN** the mutation returns HTTP 409 and leaves both structured state and conversation history unchanged

### Requirement: Requirement access is workspace-wide and collaborative

Within a North instance, authenticated users SHALL be able to view any
Requirement and its conversation. Requesters SHALL be able to create
Requirements, append conversation context, begin discussion, and edit
non-terminal Requirements. Requirement Managers, Admins, and Owners SHALL
have the same capabilities and additionally may perform reviewer-gated
transitions. North 0.1.0 SHALL NOT imply per-Requirement ACLs; human review
permissions remain a separate server-side role check.

#### Scenario: Requester collaborates on another requirement

- **WHEN** an authenticated Requester reads, converses on, begins discussion on, or edits a non-terminal Requirement they did not create
- **THEN** the operation succeeds subject only to state and expected_state_version checks

#### Scenario: Workspace visibility is not an ownership ACL

- **WHEN** any authenticated instance user requests a known Requirement id
- **THEN** the Requirement and conversation are readable without an owner-only filter

#### Scenario: Review remains role-gated

- **WHEN** a Requester attempts Accept, Reject, Request Changes, or Reopen
- **THEN** the server rejects the request before lifecycle mutation
