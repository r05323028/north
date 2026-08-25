## Purpose

Makes Ready a precise, auditable claim: an agent's verdict bound to exactly
one requirement revision, validated by the server, voided by any later edit.

## ADDED Requirements

### Requirement: Assessments bind to a revision

Every ReadinessAssessment SHALL record the requirement revision it targets,
a verdict (Ready or NeedsClarification), blockers, assumptions, and
repositories_reviewed entries carrying repository identity plus the inspected
commit SHA.

#### Scenario: Assessment cites its sources

- **WHEN** an assessment is recorded after repository inspection
- **THEN** each reviewed repository entry includes the exact commit SHA used

### Requirement: Server validates every gate before Ready

A Discussing requirement SHALL enter Ready only when a server-side validation
applies the domain gates to the submitted assessment: revision equals current,
verdict is Ready, blockers empty, acceptance criteria exist. Any failed gate
SHALL leave state unchanged.

#### Scenario: Stale event cannot promote

- **WHEN** a requirement.assessed event arrives for an older revision
- **THEN** the server refuses promotion and the requirement stays Discussing

#### Scenario: Blockers veto Ready

- **WHEN** an otherwise-valid assessment lists any blocker
- **THEN** promotion fails with the blocker gate named

### Requirement: Edits invalidate assessments automatically

After any accepted edit to a Ready requirement, its assessment SHALL be
treated as stale: status SHALL be Discussing, and promotion SHALL require a
new assessment targeting the new revision. This SHALL NOT depend on agent or
user memory.

#### Scenario: Demotion needs no bookkeeping ritual

- **WHEN** an edit lands on a Ready requirement
- **THEN** reading the requirement shows Discussing immediately and the old
assessment cannot be reused

### Requirement: Review packet projects the handoff

For a Ready requirement the system SHALL serve a concise packet containing
goal, scope, acceptance criteria, assumptions, blocking questions, and
repositories inspected — enough for review without replaying conversation.

#### Scenario: Reviewer skips the transcript

- **WHEN** a Requirement Manager opens the review view
- **THEN** all packet sections render from the latest valid assessment and
structured fields alone
