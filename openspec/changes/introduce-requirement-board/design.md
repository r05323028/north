# Design

## Decisions

- Data fetching via server API routes proxying north-server (no direct DB).
- Live refresh via SSE subscription per board/list view; optimistic create.
- shadcn/ui primitives: Card, Badge, Table, DropdownMenu, Dialog, Input,
  Select. Status → column mapping from the lifecycle enum's stable strings.
- Frontend tests start here minimally: vitest + one render test for the board
  grouping logic, establishing the pattern docs/development/testing.md points to.

## Open Questions

None.
