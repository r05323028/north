## MODIFIED Requirements

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
- **THEN** status becomes Discussing with reviewer feedback preserved, and the requirement is NOT Rejected

#### Scenario: Valid assessment reaches Ready

- **WHEN** a current-revision assessment has a Ready verdict, no blockers, and meaningful acceptance criteria
- **THEN** the server persists its typed evidence and moves the Discussing requirement to Ready atomically

#### Scenario: Invalid assessment cannot reach Ready

- **WHEN** an assessment is stale, blocked, lacks criteria, or requests clarification
- **THEN** the server durably records rejection and leaves Requirement state unchanged
