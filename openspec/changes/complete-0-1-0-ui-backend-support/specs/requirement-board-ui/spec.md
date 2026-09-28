# Spec Delta

## MODIFIED Requirements

### Requirement: Board scope excludes unrelated product features

The board/list change SHALL include only lifecycle board, list search/filter/sort,
minimal creation, the Board-owned minimal read-only Requirement detail shell,
detail navigation, live notification refetch, persistent same-column card
ordering, and drag-and-drop mapped to existing legal lifecycle operations. It
SHALL not add clarification, runtime status, activity, readiness interaction,
editing, labels, attachments, advanced prioritization, a generic status-mutation
API, or unrelated administration. Drag-and-drop SHALL preserve the domain
lifecycle: Discussing to Ready remains assessment-owned and Accepted remains
terminal.

#### Scenario: Same-column drop changes presentation order only

- **WHEN** a user reorders a card within its current lifecycle column
- **THEN** the board submits the narrow reorder operation and the Requirement's status, revision, and state_version remain unchanged

#### Scenario: Cross-column drop uses an existing legal operation

- **WHEN** a user drops a card into a different lifecycle column
- **THEN** the board invokes only the corresponding existing explicit transition operation, and any destination placement uses the same-column reorder operation after the transition succeeds

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

The board SHALL request the authenticated `GET /requirements?sort=board`
collection for deterministic lifecycle-column ordering. A same-column reorder
SHALL use `POST /requirements/{id}/reorder` with `expected_state_version` and
optional `before_id` and `after_id` neighbors. The neighbor pair SHALL describe
the card's final insertion point after removing it from the current column;
null denotes a column boundary. The operation SHALL NOT accept a target status.
A successful legal lifecycle transition SHALL place the card at the end of its
destination column, after which a same-column reorder MAY place it among its
new neighbors.

#### Scenario: Explicit transition mapping remains authoritative

- **WHEN** a drag targets Draft to Discussing, Ready to Accepted, Ready to Rejected, Ready to Discussing, or Rejected to Discussing
- **THEN** the board uses respectively begin-discussion, accept, reject, request-changes with required feedback, or reopen; role, readiness, and expected-version guards remain enforced by those existing operations

#### Scenario: Reorder rejects lifecycle mutation

- **WHEN** a reorder request attempts to name or imply a destination lifecycle status
- **THEN** the server rejects it because the reorder API changes presentation order only

### Requirement: Board updates use existing HTTP and SSE repair path

After a committed reorder changes visible board order, the server SHALL emit the
existing identity-only `requirement.changed` notification. The browser SHALL
refetch the canonical board-sorted HTTP collection; the notification SHALL not
carry rank or become a second state source. The system SHALL NOT add browser
WebSockets or another realtime producer.

#### Scenario: Reorder hint repairs canonical order

- **WHEN** a reorder commits while a board is open
- **THEN** the existing SSE hint triggers a canonical board-sorted HTTP refetch that returns the persisted order
