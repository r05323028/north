# Design

## Decisions

- Data fetching via server API routes proxying north-server (no direct DB).
- Live refresh uses an SSE subscription per board/list view. Events are
  notification hints; EventSource reconnect and optional `Last-Event-ID` may
  suppress redundant hints, but every reconnect/refocus can refetch canonical
  server state. No durable browser event log or WebSocket.
- shadcn/ui primitives: Card, Badge, Table, DropdownMenu, Dialog, Input,
  Select. Status → column mapping from the lifecycle enum's stable strings.
- Frontend tests start here minimally: vitest + one render test for the board
  grouping logic, establishing the pattern docs/development/testing.md points to.

## Open Questions

None.
