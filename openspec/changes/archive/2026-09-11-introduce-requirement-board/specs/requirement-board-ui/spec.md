# requirement-board-ui Specification

## Purpose

Gives users fast orientation through a lifecycle board and precise lookup
through a server-backed list, without making browser notifications a source of
Requirement truth.

## ADDED Requirements

### Requirement: Board and list use the existing collection API at 0.1 scale

The board and list SHALL consume the current authenticated
`GET /requirements` collection and SHALL use its existing `search`, `status`,
`created_by`, and `sort` query parameters. North 0.1.0 defines no cursor,
`limit`, `offset`, total-count, page-size, or virtualization contract for this
surface. Cursor pagination and virtualization SHALL remain out of scope unless
a separate change first defines them. The HTTP representation of `revision` and
`state_version` remains numeric; the browser boundary SHALL accept only positive
JavaScript safe integers (`Number.isSafeInteger(value) && value >= 1`).

#### Scenario: Query controls stay server-backed

- **WHEN** a user enters search text or selects status, creator, or updated sort
- **THEN** the UI sends the corresponding supported query parameters and renders the returned collection without a client-side full-collection filter pass

#### Scenario: No page-size promise is implied

- **WHEN** the current collection grows within the single-instance 0.1 product scope
- **THEN** the UI consumes the array returned by the existing endpoint and makes no claim that behavior is invariant under an undefined page size

### Requirement: Board owns minimal Requirement detail route

The Board change SHALL provide the read-only `/requirements/[id]` route reached
from board, list, and creation flows. It SHALL load the existing authenticated
`GET /requirements/{id}` response and render only canonical Requirement fields,
including title, description, status, creator (`created_by`), updated time,
summary, acceptance criteria, assumptions, open questions, revision, and
state_version where useful. The base shell SHALL not require clarification or
runtime availability and SHALL not add clarification controls, activity,
readiness interaction, or editing.

#### Scenario: Board detail route works without clarification

- **WHEN** board, list, or creation navigation opens `/requirements/[id]` before clarification is shipped or while it is unavailable
- **THEN** the route loads `GET /requirements/{id}` and renders a valid read-only Requirement detail shell from that response

#### Scenario: Base detail uses canonical creator data

- **WHEN** the detail response includes `created_by`
- **THEN** the shell renders that canonical creator value and does not infer a Requirement owner or assignee field

### Requirement: Board groups by lifecycle state

The board SHALL render one fixed column for each canonical server status
identifier: `draft`, `discussing`, `ready`, `accepted`, and `rejected`. The
corresponding display labels SHALL be Draft, Discussing, Ready, Accepted, and
Rejected. Cards SHALL be placed using the server-reported identifier and SHALL
show at least title, its presentation status label, creator (`created_by`), and
updated time. A create action and navigation to the Board-owned Requirement
detail route SHALL be available.

#### Scenario: Column placement matches server state

- **WHEN** the collection contains Requirements with mixed lifecycle statuses
- **THEN** every returned Requirement appears in exactly one column keyed by its canonical server status identifier and displays that identifier's presentation label

### Requirement: List supports server search, filters, and sorting

The list SHALL expose text search, status and creator filters, and updated-time
sorting with the existing server query contract. Its creator filter SHALL use the
existing `created_by` query parameter. It SHALL render status and creator
(`created_by`) columns and navigate to the same detail route. It SHALL not
invent Requirement owner or assignee fields.

#### Scenario: Status filter narrows returned data

- **WHEN** a user selects Ready
- **THEN** the UI requests `status=ready` and renders only rows returned by the server for that query

### Requirement: Creation is minimal and canonical

The creation flow SHALL require only title and description, submit through the
existing create endpoint, and use the returned Requirement as canonical data.
It SHALL navigate to the created Requirement without extra wizard steps or
client-predicted lifecycle/version values.

#### Scenario: Two fields reach a Draft

- **WHEN** a user submits valid title and description
- **THEN** the server-created Draft is rendered/navigated to using its returned ID, status, revision, and state_version

### Requirement: Browser transport is HTTP plus notification-only SSE

Board and list SHALL use HTTP for canonical reads/mutations and this change's
single authenticated `GET /events` SSE endpoint for lightweight invalidation
hints. The base producer SHALL emit `requirement.changed` only after the
canonical Requirement transaction commits. `Last-Event-ID` SHALL not be
required for correctness. If the server detects that an SSE subscriber missed
broadcast notifications, it SHALL close that stream so native EventSource
reconnect/refetch can restore canonical state. `introduce-agent-requirement-clarification` may
extend this same producer with clarification categories, but SHALL not create
another endpoint or producer. The frontend SHALL never open a WebSocket.

The invalidation path SHALL be:

```text
canonical north-server commit
  -> SSE notification (requirement identity/category only)
  -> GET /requirements refetch
  -> render canonical response
```

#### Scenario: Canonical status change refreshes a board

- **WHEN** a server-side action changes a visible Requirement's status and emits a `requirement.changed` hint
- **THEN** an open board/list refetches canonical HTTP data and reflects the returned status without applying the hint as a state patch

#### Scenario: Base SSE does not depend on clarification

- **WHEN** the board/list is used before clarification runtime is available
- **THEN** the authenticated `GET /events` endpoint can still deliver `requirement.changed` hints and the board/list can refetch `GET /requirements`

### Requirement: Missed and duplicate hints are harmless

SSE SHALL not be a durable browser event log, a Requirement state store, a
WebSocket transport, or a required replay mechanism. After initial load,
refocus, disconnect, or EventSource reconnect, board/list SHALL refetch
`GET /requirements`. Missed, duplicated, delayed, or out-of-order hints SHALL
never duplicate a Requirement row or lifecycle transition. If the server detects
that a subscriber lagged beyond the broadcast buffer, it closes the stream so
EventSource reconnect/refetch restores canonical state.

#### Scenario: Missed update is repaired

- **WHEN** the browser misses a notification while offline
- **THEN** reconnect/refocus HTTP refetch returns the current server collection without replaying stream history

#### Scenario: Lagged subscriber reconnects

- **WHEN** the server detects that an SSE subscriber has missed broadcast notifications
- **THEN** the server terminates that stream, native EventSource reconnects, and the browser refetches canonical HTTP state without requiring replay

#### Scenario: Duplicate hint does not duplicate state

- **WHEN** the same notification arrives twice
- **THEN** the UI may coalesce or perform harmless repeated refetches, but it never duplicates a row or derives a second transition

### Requirement: Board scope excludes unrelated product features

The board/list change SHALL include only lifecycle board, list search/filter/sort,
minimal creation, the Board-owned minimal read-only Requirement detail shell,
detail navigation, live notification refetch, and frontend test foundation. It
SHALL not add clarification, runtime status, activity, readiness interaction,
editing, labels, attachments, advanced prioritization, drag-and-drop lifecycle
mutation, or unrelated administration.

#### Scenario: Card actions do not mutate lifecycle by drag

- **WHEN** a user reorders or drags a card in the board
- **THEN** no unrequested lifecycle mutation API is invoked; lifecycle changes remain server/domain operations outside this surface
