## Purpose

Turns Ready into a decisive human moment: a compact evidence packet, three
explicit outcomes, and structural protection against approving outdated work.

## ADDED Requirements

### Requirement: Packet-first review surface

Reviewing a Ready requirement SHALL present Goal, Scope, Acceptance Criteria,
Assumptions, Blocking Questions, and Repositories Inspected without requiring
the reviewer to read the conversation.

#### Scenario: Five-minute review is possible

- **WHEN** a reviewer opens a Ready requirement's review view
- **THEN** all six sections render from the current Requirement plus the latest revision-matched assessment

### Requirement: Reviewer-only decisions with preserved feedback

Accept, Reject, and Request Changes SHALL be restricted to Requirement
Manager/Admin/Owner server-side. Request Changes SHALL capture reviewer
feedback and return the requirement to Discussing; Reopen SHALL return
Rejected requirements to Discussing.

#### Scenario: Feedback survives request-changes

- **WHEN** a reviewer requests changes with comments
- **THEN** the requirement lands Discussing with those comments attached to
its history

### Requirement: Stale assessments cannot be approved

Before executing any review decision, the system SHALL verify the requirement
revision still matches the assessment backing the displayed packet; a moved
revision SHALL refuse the action and require a fresh read.

#### Scenario: Race against an edit loses safely

- **WHEN** a requester edits the requirement after the reviewer loaded it but
before clicking Accept
- **THEN** the accept fails with a staleness error and nothing is approved

### Requirement: Decisions leave an audit trail

Every decision SHALL record who decided, what, and when, queryable alongside
the requirement history.

#### Scenario: Accountability lookup

- **WHEN** anyone inspects an Accepted requirement later
- **THEN** the accepting reviewer and timestamp are retrievable

### Requirement: Review mutations require expected_state_version

Accept, Reject, Request Changes, and Reopen SHALL require `expected_state_version`.
Accept, Reject, and Request Changes SHALL also carry the packet's `assessment_id`.
The server SHALL atomically compare state_version, current content revision,
Ready state generation, and the current accepted assessment identity before
applying the domain operation. Reopen needs no assessment_id. A mismatch SHALL
return HTTP 409 and SHALL write no decision, feedback, audit row, or lifecycle
transition.

#### Scenario: Review race loses safely

- **WHEN** a reviewer submits Accept for state_version 12 or assessment_id A after a newer Ready generation commits
- **THEN** the server returns HTTP 409 and no approval or audit row is written
