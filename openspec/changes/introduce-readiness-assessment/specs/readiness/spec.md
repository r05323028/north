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

### Requirement: Review packet projects Requirement plus assessment

For a Ready requirement the system SHALL serve a concise packet that is a
projection of TWO sources: goal/title, scope/description, summary, acceptance
criteria, assumptions, and open questions from the canonical Requirement (source of truth
for content); blockers, assessment assumptions, and repositories-inspected
(commit SHAs) from the latest ReadinessAssessment bound to exactly that
revision. Projection SHALL refuse any revision mismatch so a stale packet is
never reviewable or acceptable. The packet SHALL NOT be stored as truth.

#### Scenario: Reviewer skips the transcript

- **WHEN** a Requirement Manager opens the review view
- **THEN** all six sections render from the structured fields plus the
revision-matched assessment alone

#### Scenario: Stale pair cannot render a reviewable packet

- **WHEN** the requirement revision no longer matches the assessment backing
the packet request
- **THEN** projection fails with staleness and no packet is served

### Requirement: Assessment ingestion acknowledges only a committed result

For `requirement.assessed`, the server SHALL deduplicate the event, load/lock
the current Requirement, validate the event revision, run domain readiness
gates, persist immutable evidence with its accepted/rejected result, apply any
valid transition, persist the resulting row, and commit as one transaction.
Only after commit SHALL it send `event.accepted` for a valid effect or
`event.rejected` for a durable rejection. A stale/invalid event SHALL not
change Requirement state; a duplicate of a committed event SHALL not repeat
its effect.

#### Scenario: Event ACK follows durable assessment handling

- **WHEN** the server receives a current-revision assessment or a stale assessment
- **THEN** it commits the corresponding effect or rejection/dedupe record before sending the matching event ACK
