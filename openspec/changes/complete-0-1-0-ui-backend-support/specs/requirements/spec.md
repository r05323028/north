# Spec Delta

## MODIFIED Requirements

### Requirement: Queryable list with deterministic ordering

The API SHALL support text search, status/creator filters, and updated-time sorting without client scans, preserving existing default and updated-sort behavior. Add `sort=board`, ordered by lifecycle status (Draft, Discussing, Ready, Accepted, Rejected) then persisted rank; equal or missing ranks SHALL use creation time and requirement ID as deterministic tie-breakers.

#### Scenario: Board feeds itself from the API

- **WHEN** a client requests requirements grouped by status
- **THEN** results are complete and ordered deterministically per sort key

#### Scenario: Board sort is stable across reads

- **WHEN** a client requests `GET /requirements?sort=board` repeatedly
- **THEN** each lifecycle column's requirements are returned in the same persisted order, with deterministic fallback ordering for equal or missing ranks

## ADDED Requirements

### Requirement: Board position is durable presentation state

The system SHALL persist card order as presentation state separate from the Requirement aggregate and maintain a valid position per card and lifecycle column. Creation and legal lifecycle transitions SHALL place cards at the destination-column end; reorders SHALL NOT change lifecycle status, content, revision, state_version, readiness validity, or human-review evidence.

#### Scenario: Reordering leaves Requirement identity unchanged

- **WHEN** a client changes a card's position within its lifecycle column
- **THEN** the persisted order changes while requirement status, content, revision, state_version, readiness evidence, and review evidence remain unchanged

#### Scenario: Lifecycle transition receives destination position

- **WHEN** an existing explicit lifecycle operation changes a requirement's status
- **THEN** the operation also gives that card a valid position in the destination column without exposing arbitrary status mutation

#### Scenario: Rank collision and missing legacy position remain deterministic

- **WHEN** board ranks collide or a requirement has no position row
- **THEN** board reads still return a stable order using deterministic fallback keys
