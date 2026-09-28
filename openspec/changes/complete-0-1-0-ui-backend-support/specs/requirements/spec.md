# Spec Delta

## MODIFIED Requirements

### Requirement: Queryable list with deterministic ordering

The system SHALL expose listing with search over text fields, filtering by
status and creator, and sorting by updated time — sufficient for board and list
views without client-side full scans. It SHALL also support the additive
`sort=board` option, which orders records by canonical lifecycle status and
persisted presentation rank. Existing default and updated-time sort behavior
SHALL remain unchanged. Board ordering SHALL use the status order Draft,
Discussing, Ready, Accepted, Rejected, then rank ascending; equal or missing
ranks SHALL use deterministic creation-time and requirement-id tie-breakers.

#### Scenario: Board feeds itself from the API

- **WHEN** a client requests requirements grouped by status
- **THEN** results are complete and ordered deterministically per sort key

#### Scenario: Board sort is stable across reads

- **WHEN** a client requests `GET /requirements?sort=board` repeatedly
- **THEN** each lifecycle column's requirements are returned in the same persisted order, with deterministic fallback ordering for equal or missing ranks

## ADDED Requirements

### Requirement: Board position is durable presentation state

The system SHALL persist requirement card order separately from the Requirement domain aggregate and SHALL provide a valid position for every requirement in its current lifecycle column. Creating a requirement or applying an existing legal lifecycle transition SHALL place the card at the end of its destination column unless a subsequent reorder positions it elsewhere. A board-order change SHALL NOT change lifecycle status, structured content, revision, state_version, readiness assessment validity, or human-review evidence.

#### Scenario: Reordering leaves Requirement identity unchanged

- **WHEN** a client changes a card's position within its lifecycle column
- **THEN** the persisted order changes while requirement status, content, revision, state_version, readiness evidence, and review evidence remain unchanged

#### Scenario: Lifecycle transition receives destination position

- **WHEN** an existing explicit lifecycle operation changes a requirement's status
- **THEN** the operation also gives that card a valid position in the destination column without exposing arbitrary status mutation

#### Scenario: Rank collision and missing legacy position remain deterministic

- **WHEN** board ranks collide or a requirement has no position row
- **THEN** board reads still return a stable order using deterministic fallback keys
