# Design

## Backend contract consumed

Use the existing authenticated Requirement API rather than adding a board-only
read layer:

```text
GET  /requirements
     ?search=<text>&status=<draft|discussing|ready|accepted|rejected>
     &created_by=<user-id>&sort=updated|updated_asc
POST /requirements       { title, description }
GET  /requirements/{id}
```

Requirement `status` values on the wire and in the frontend domain are the
lowercase identifiers `draft`, `discussing`, `ready`, `accepted`, and
`rejected`. The corresponding presentation labels are `Draft`, `Discussing`,
`Ready`, `Accepted`, and `Rejected`. The HTTP wire format keeps
`revision` and `state_version` as JSON numbers; the browser boundary accepts
only positive JavaScript safe integers (`Number.isSafeInteger(value) && value >= 1`).
No string-token redesign is part of North 0.1.

`GET /requirements` returns the current single-instance collection as one JSON
array. Its server-side search covers structured Requirement fields, status and
creator filters are server-side, and updated sorting has a deterministic ID
tiebreak. The UI must not assume a cursor, `limit`, `offset`, total count, or
page-size invariant that the API does not define.

## Views

- Board maps canonical server status identifiers (`draft`, `discussing`,
  `ready`, `accepted`, `rejected`) to fixed columns labeled Draft, Discussing,
  Ready, Accepted, and Rejected. A card renders title, the presentation status
  label, creator (`created_by`), and updated timestamp and links to
  `/requirements/[id]`.
- List preserves server order, renders search/filter/sort controls, and sends
  each control as query parameters. Its creator filter uses the existing
  `created_by` query parameter; it does not fetch the full collection and
  filter it in the browser.
- The Board-owned detail shell at `/requirements/[id]` reads the existing
  `GET /requirements/{id}` response and renders canonical title, description,
  status, creator, updated time, summary, acceptance criteria, assumptions,
  open questions, revision, and state_version where useful. It is read-only and
  does not require clarification, runtime status, activity, readiness
  interaction, or editing.
- Create sends only title and description, uses the returned Requirement as
  canonical, and navigates to the Board-owned detail shell. No wizard or
  optimistic lifecycle prediction is needed.

## Shared browser notification path

This change owns one authenticated `GET /events` SSE producer/endpoint and its
initial `requirement.changed` category. It emits only a lightweight hint after
the canonical Requirement transaction commits. Board/list fetches
`GET /requirements` on initial load, focus/refocus, and EventSource reconnect.
On any notification, they may refetch once or coalesce nearby hints, but never
patch a card from SSE data. `Last-Event-ID` is not a correctness contract; no
browser stream replay is required. If the server detects that an SSE subscriber
missed broadcast notifications, it closes that stream so native EventSource
reconnect/refetch restores canonical state. Producer failures do not roll back
canonical server mutations.

`introduce-agent-requirement-clarification` extends this same endpoint with
`conversation.changed`, `readiness.changed`, `activity.changed`, and
`session.changed` after its corresponding canonical transactions. It does not
create another endpoint, event bus, browser event store, or WebSocket path.

## Explicit boundary

Board/list and the minimal read-only detail shell own browser rendering, query
state, subscription, base `/events` transport, and canonical refetch. The route
`/requirements/[id]` is owned here and reads only the existing Requirement
endpoint. Clarification owns its canonical runtime read models and extends the
shared notification categories. No browser WebSocket, daemon connection,
durable browser event store, or second SSE producer is added.

## Test foundation

Use the existing web test/lint stack. Start with a pure grouping test for mixed
Requirement statuses, an interaction test proving query controls map to server
parameters, and a route test proving `/requirements/[id]` renders from
`GET /requirements/{id}` without clarification data. Add an end-to-end fixture
that drops and duplicates hints, then verifies HTTP refetch restores the current
collection. The Board/detail slice remains runnable when clarification is
unavailable or not shipped.
