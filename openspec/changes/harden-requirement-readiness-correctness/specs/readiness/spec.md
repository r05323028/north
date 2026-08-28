## MODIFIED Requirements

### Requirement: Server validates every gate before Ready

A Discussing requirement SHALL enter Ready only when server-side validation
applies the domain gates to a uniquely identified, sequence-valid assessment:
revision equals current, verdict is Ready, blockers empty, and acceptance
criteria exist. Any failed readiness gate SHALL leave status, revision, and
`state_version` unchanged while recording a handled rejection. An event identity
or per-session sequence conflict SHALL be rejected as a protocol error before
evidence insertion; it SHALL not mutate Requirement state or emit an ACK.
A successful promotion SHALL increment `state_version` exactly once in the same
transaction; it SHALL NOT change `revision`.

#### Scenario: Stale event cannot promote

- **WHEN** a requirement.assessed event arrives for an older revision
- **THEN** the server refuses promotion and the requirement stays Discussing with both version tokens unchanged

#### Scenario: Blockers veto Ready

- **WHEN** an otherwise-valid assessment lists any blocker
- **THEN** promotion fails with the blocker gate named and state_version is unchanged

#### Scenario: Promotion increments mutable state once

- **WHEN** a current assessment passes all gates for a Discussing requirement
- **THEN** the requirement becomes Ready with the same revision and state_version previous+1

#### Scenario: Duplicate promotion is idempotent

- **WHEN** the same committed assessment event is delivered again
- **THEN** the server returns the prior durable result and does not increment state_version again

### Requirement: Edits invalidate assessments automatically

After any accepted edit to a Ready requirement, its assessment SHALL be treated
as stale: status SHALL be Discussing, `revision` and `state_version` SHALL each
increment once, and promotion SHALL require a new assessment targeting the new
revision. No-op edits SHALL change neither token or status. This SHALL NOT
depend on agent or user memory.

#### Scenario: Demotion needs no bookkeeping ritual

- **WHEN** an edit lands on a Ready requirement
- **THEN** reading the requirement shows Discussing immediately and the old assessment cannot be reused

### Requirement: Review packet projects Requirement plus assessment

For a Ready requirement the system SHALL serve a concise packet that is a
projection of TWO sources: goal/title, scope/description, summary, acceptance
criteria, assumptions, and open questions from the canonical Requirement;
blockers, assessment assumptions, and repositories-inspected commit SHAs from
the latest accepted ReadinessAssessment bound to exactly that revision. The
accepted evidence SHALL record the `state_version` created by its successful
Ready promotion. The packet SHALL include `requirement_revision`,
`requirement_state_version`, and stable `assessment_id`; persistence SHALL select
accepted evidence for the exact current state generation, not timestamp order.
Projection SHALL refuse any revision, state-generation, Ready-state, or
assessment-identity mismatch so a stale packet is never reviewable or
acceptable. The packet SHALL NOT be stored as truth.

#### Scenario: Reviewer skips the transcript

- **WHEN** a Requirement Manager opens the review view
- **THEN** all structured sections and assessment evidence render from the packet's two current sources alone

#### Scenario: Stale pair cannot render a reviewable packet

- **WHEN** the requirement revision or Ready state generation no longer matches the assessment backing the packet request
- **THEN** projection fails with staleness and no packet is served

#### Scenario: Review decision names reviewed evidence

- **WHEN** a reviewer submits Accept, Reject, or Request Changes
- **THEN** the server requires the packet's assessment_id and expected_state_version and rejects a replaced assessment with HTTP 409 before mutation

### Requirement: Assessment ingestion acknowledges only a committed result

For `requirement.assessed`, the server SHALL require the event session to be
bound to the payload Requirement, validate event identity, deduplicate the event,
and detect per-session sequence conflicts before loading/locking the current
Requirement. Identity or sequence conflicts SHALL be protocol errors with no
assessment row or event ACK. For a uniquely identified, sequence-valid event,
the server SHALL load/lock the current Requirement, validate the event revision,
run domain readiness gates, persist immutable evidence with its
accepted/rejected result, record the post-promotion `state_version` on an
accepted assessment, apply any valid transition and exactly one `state_version`
increment, and commit as one transaction. Only after commit SHALL it send
`event_ack(status=accepted)` for a valid effect or
`event_ack(status=rejected)` for a durable rejection. A stale/invalid event SHALL
not change Requirement revision, state_version, or status; a duplicate of a
committed event SHALL not repeat its effect or version increment.

#### Scenario: Event ACK follows durable assessment handling

- **WHEN** the server receives a current-revision assessment or a stale assessment
- **THEN** it commits the corresponding effect or rejection/dedupe record before sending the matching event ACK

#### Scenario: Session cannot assess another requirement

- **WHEN** an authenticated daemon sends an assessment for a Requirement different from its bound session
- **THEN** the server rejects the event before any evidence, version, audit, or lifecycle state changes

#### Scenario: Sequence conflict is a protocol rejection

- **WHEN** a session reuses an event sequence for different assessment identity or payload
- **THEN** processing fails without inserting evidence, mutating Requirement state or state_version, or emitting an event ACK
