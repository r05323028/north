## Purpose

Makes concurrent Requirement mutations and readiness-event ingestion explicit, so stale clients cannot overwrite newer business state and event acknowledgements cannot outrun durable commits.

## ADDED Requirements

### Requirement: Existing Requirement mutations require an expected revision

Every state-changing operation on an existing Requirement SHALL carry
`expected_revision`, including structured edits, human review decisions,
agent readiness assessment application, request-changes/reopen operations, and
any future mutation endpoint. The server/persistence boundary SHALL compare the
expected value atomically with the current row revision. A mismatch SHALL
return a typed stale-revision conflict, normally HTTP `409 Conflict`, with no
state, audit, message, assessment, or retry side effect. Requirement creation
has no expected revision because no prior row exists.

#### Scenario: Stale structured edit loses safely

- **WHEN** a client loaded revision 12, another actor committed revision 13, and the client submits `expected_revision = 12`
- **THEN** the API returns HTTP 409 and persists no part of the stale edit

#### Scenario: Stale review decision cannot approve

- **WHEN** a reviewer submits a decision for a packet whose requirement revision has moved
- **THEN** the server returns a revision conflict and records no decision or lifecycle transition

### Requirement: Assessment ingest is one atomic durable transaction

For `requirement.assessed`, the server SHALL execute one transaction that
(1) deduplicates the event id, (2) loads and locks or atomically claims the
current Requirement, (3) validates the event's requirement revision, (4) runs
domain readiness gates, (5) persists immutable assessment evidence with its
accepted/rejected validation result, (6) applies any valid lifecycle
transition, and (7) persists the resulting Requirement state. A well-formed
stale or invalid event commits its rejection/dedupe evidence without changing
Requirement state. The transaction SHALL commit before the server
acknowledges the daemon event. A rolled-back transaction SHALL leave no
dedupe marker, evidence, transition, or acknowledgement. A duplicate of a
committed event SHALL produce an acknowledgement without repeating steps 4–7.

#### Scenario: Valid assessment commits before ACK

- **WHEN** a current-revision Ready assessment passes domain gates
- **THEN** evidence and the lifecycle transition commit atomically, and only then does the server acknowledge the event

#### Scenario: Stale assessment has no partial evidence

- **WHEN** an assessment event targets revision 12 while the locked Requirement is revision 13
- **THEN** the server commits rejected assessment/dedupe evidence without a Requirement transition, then sends `event.rejected` after commit

### Requirement: Durable ACKs never claim an uncommitted effect

Every server or daemon acknowledgement that represents a durable business
effect SHALL be emitted only after the transaction containing that effect has
committed. Transport receipt, durable inbox acceptance, and business-effect
commit SHALL use distinct terms and frames so a reconnect cannot mistake one
boundary for another.

#### Scenario: Crash before commit does not consume an event

- **WHEN** the server loses process state before the assessment transaction commits
- **THEN** no success acknowledgement is observable and the daemon can replay the event without being told its effect was durable
