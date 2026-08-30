# readiness Specification

## Purpose

Makes Ready a precise, auditable claim: an agent's verdict bound to exactly one
content revision and one mutable Ready state generation, validated by server.

## Requirements

### Requirement: Assessments bind to a revision

Every wire `requirement.assessed` SHALL record the requirement revision it
targets, a typed verdict (`Ready` or `NeedsClarification`), blockers,
assumptions, and `repositories_reviewed` entries carrying non-empty repository
identity plus the inspected commit SHA. The server SHALL explicitly convert
these transport values to the domain assessment; wire types SHALL remain
domain-independent. Assessment matching SHALL use `requirement_revision`, not
mutable `state_version`, `assessment_id`, or `accepted_state_version`; those
identities are not inbound assessment concurrency tokens. Accepted readiness
evidence creates/binds `assessment_id` and records the resulting Ready-generation
`accepted_state_version`.

#### Scenario: Assessment cites its sources

- **WHEN** an assessment is recorded after repository inspection
- **THEN** each reviewed repository entry includes the exact commit SHA used

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
Ready promotion. The packet SHALL include `assessment_id`,
`requirement_revision`, and `requirement_state_version`; persistence SHALL
select accepted evidence for the exact current state generation, not timestamp
order. Projection SHALL refuse any revision, state-generation, Ready-state, or
assessment-identity mismatch so a stale packet is never reviewable or
acceptable. Equality between `accepted_state_version` and the Requirement's
current `state_version` is required only while that exact Ready generation is
current and reviewable. A later human review transition increments the
Requirement token without mutating historical assessment evidence. The packet
SHALL NOT be stored as truth.

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
the server SHALL load/lock the current Requirement, validate
`requirement_revision` against its current revision, verify every cited
`repository_id` identifies an existing durable configured-repository row and was
valid for the session/run context, run domain readiness gates, persist immutable
evidence with its accepted/rejected result, record the post-promotion
`state_version` as `accepted_state_version` on accepted evidence, apply any valid
transition and exactly one `state_version` increment, and commit as one
transaction. `assessment_id` and `accepted_state_version` are created/bound by
accepted readiness evidence, not supplied as inbound concurrency tokens. Only
after commit SHALL it send `event_ack(status=accepted)` for a valid effect or
`event_ack(status=rejected)` for a durable rejection. A stale/invalid event SHALL
not change Requirement revision, state_version, or status; a duplicate of a
committed event SHALL not repeat its effect or version increment. The
`north-protocol` wire layer validates repository identity and complete Git
SHA-1/SHA-256 fields; repository existence and session/run acceptability belong
to server readiness persistence.

#### Scenario: Event ACK follows durable assessment handling

- **WHEN** the server receives a current-revision assessment or a stale assessment
- **THEN** it commits the corresponding effect or rejection/dedupe record before sending the matching event ACK

#### Scenario: Session cannot assess another requirement

- **WHEN** an authenticated daemon sends an assessment for a Requirement different from its bound session
- **THEN** the server rejects the event before any evidence, version, audit, or lifecycle state changes

#### Scenario: Unknown repository citation is durably rejected

- **WHEN** an otherwise well-formed assessment cites a non-empty unknown
  `repository_id`
- **THEN** the server commits a durable rejection, does not accept the evidence
  or promote the Requirement, and does not fabricate a repository row

#### Scenario: Disabled in-flight repository citation remains eligible

- **WHEN** repository R was enabled in `session.start`, inspection began, R was
  then disabled, and the assessment cites R with the exact commit SHA from that
  authorized run
- **THEN** readiness does not reject the citation solely because R was disabled;
  the durable row and session/run validity remain required, and R remains
  unavailable for new inspection selection

#### Scenario: Sequence conflict is a protocol rejection

- **WHEN** a session reuses an event sequence for different assessment identity or payload
- **THEN** processing fails without inserting evidence, mutating Requirement state or state_version, or emitting an event ACK
