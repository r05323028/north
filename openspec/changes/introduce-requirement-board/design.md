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

`GET /requirements` returns the current single-instance collection as one JSON
array. Its server-side search covers structured Requirement fields, status and
creator filters are server-side, and updated sorting has a deterministic ID
tiebreak. The UI must not assume a cursor, `limit`, `offset`, total count, or
page-size invariant that the API does not define.

## Views

- Board maps the stable server status strings Draft, Discussing, Ready,
  Accepted, and Rejected to fixed columns. A card renders title, status,
  requester, and updated timestamp and links to `/requirements/[id]`.
- List preserves server order, renders search/filter/sort controls, and sends
  each control as query parameters. It does not fetch the full collection and
  filter it in the browser.
- Create sends only title and description, uses the returned Requirement as
  canonical, and navigates to its detail route. No wizard or optimistic
  lifecycle prediction is needed.

## Shared notification path

`introduce-agent-requirement-clarification` owns one authenticated
`GET /events` SSE producer/endpoint. Board/list subscribe to the categories
relevant to collection invalidation, at minimum `requirement.changed`; the
server may also publish `conversation.changed`, `readiness.changed`,
`activity.changed`, and `session.changed` for the detail UI.

On initial load, focus/refocus, and EventSource reconnect, board/list fetch
`GET /requirements` again. On any notification, they may refetch once or
coalesce nearby hints, but never patch a card from SSE data. `Last-Event-ID` is
not a correctness contract; no browser stream replay is required. Producer
failures do not roll back canonical server mutations.

## Explicit boundary

Board/list owns browser rendering, query state, subscription, and refetch. The
clarification change owns the server notification source and canonical runtime
read models. No browser WebSocket, daemon connection, durable browser event
store, or second SSE producer is added.

## Test foundation

Use the existing web test/lint stack. Start with a pure grouping test for mixed
Requirement statuses and an interaction test proving query controls map to
server parameters. Add an end-to-end fixture that drops and duplicates hints,
then verifies HTTP refetch restores the current collection.
