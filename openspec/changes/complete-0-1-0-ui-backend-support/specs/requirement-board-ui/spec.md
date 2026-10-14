# Spec Delta

## MODIFIED Requirements

### Requirement: Board scope excludes unrelated product features

The board/list change SHALL include only lifecycle board, list search/filter/sort, minimal creation, Board-owned minimal read-only Requirement detail/navigation, live notification refetch, persistent same-column ordering, and drags through existing legal lifecycle operations. It SHALL exclude clarification, runtime status, activity, readiness interaction, editing, labels, attachments, advanced prioritization, generic status mutation, and unrelated administration.

#### Scenario: Same-column drop changes presentation order only

- **WHEN** a user reorders a card within its current lifecycle column
- **THEN** the board submits the narrow reorder operation and the Requirement's status, revision, and state_version remain unchanged

#### Scenario: Cross-column drop uses an existing legal operation

- **WHEN** a user drops a card into a different lifecycle column
- **THEN** the board permits only Draft to Discussing via begin-discussion or Rejected to Discussing via reviewer reopen, using that existing operation before same-column destination reorder; readiness and Ready review decisions remain assessment/human-review owned, and Accepted remains terminal

#### Scenario: Discussing cannot be dragged to Ready

- **WHEN** a user attempts to drag a Discussing card directly into Ready
- **THEN** no manual readiness transition is sent and the server-authoritative status remains Discussing until readiness assessment succeeds

#### Scenario: Accepted remains terminal

- **WHEN** a user attempts to drag an Accepted card to another column
- **THEN** the attempt is rejected and Accepted remains unchanged

#### Scenario: Card actions do not mutate lifecycle by drag

- **WHEN** a user reorders or drags a card in the board
- **THEN** no unrequested lifecycle mutation API is invoked; lifecycle changes remain server/domain operations outside this surface

## ADDED Requirements

### Requirement: Board ordering uses a narrow canonical API

The board SHALL read the authenticated `GET /requirements?sort=board` collection and reorder within a column via `POST /requirements/{id}/reorder` with `expected_state_version` and optional `before_id`/`after_id` anchors. Anchors SHALL identify the final insertion point after removal; null denotes a column boundary. Reorder SHALL NOT accept a target status. A successful legal transition SHALL append the card to its destination column before a later same-column reorder.

#### Scenario: Explicit transition mapping remains authoritative

- **WHEN** a drag targets Draft to Discussing or Rejected to Discussing
- **THEN** the board uses respectively begin-discussion or reviewer reopen; role and expected-version guards remain enforced by those existing operations; Ready review decisions remain in the requirement-review-panel because Board drag supplies neither review packet nor required feedback

#### Scenario: Reorder rejects lifecycle mutation

- **WHEN** a reorder request attempts to name or imply a destination lifecycle status
- **THEN** the server rejects it because the reorder API changes presentation order only

#### Scenario: Failed reorder restores canonical board state

- **WHEN** reorder or an allowed transition fails, including a stale-version conflict
- **THEN** the board displays a clear error, refetches `GET /requirements?sort=board`, and renders only server-confirmed ordering and lifecycle state

#### Scenario: Unsupported drop is rejected without mutation

- **WHEN** a card is dropped across columns other than Draft to Discussing or Rejected to Discussing
- **THEN** the board sends no lifecycle mutation and refreshes canonical board data

### Requirement: Board updates use existing HTTP and SSE repair path

After a committed reorder changes visible board order, the server SHALL emit the
existing identity-only `requirement.changed` notification. The browser SHALL
refetch the canonical board-sorted HTTP collection; the notification SHALL not
carry rank or become a second state source. The system SHALL NOT add browser
WebSockets or another realtime producer.

#### Scenario: Reorder hint repairs canonical order

- **WHEN** a reorder commits while a board is open
- **THEN** the existing SSE hint triggers a canonical board-sorted HTTP refetch that returns the persisted order
