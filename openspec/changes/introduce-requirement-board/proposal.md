# Introduce Requirement Board and List

## Why

Requesters need a fast view of Requirement work and a precise way to find it.
Board and list are the smallest requester-facing surface for that workflow.

## Existing backend capability

The current `north-server` Requirement API already provides the backend this UI
needs:

- `POST /requirements` creates a Draft from title and description;
- `GET /requirements/{id}` retrieves a complete Requirement;
- `GET /requirements` supports server-side `search`, `status`, `created_by`,
  and updated-time sorting; and
- responses include canonical lifecycle status, `revision`, and
  `state_version`.

This change does not reimplement, paginate, or redesign those APIs.

## What Changes

- Next.js Board view with one column per lifecycle state, compact cards, create
  action, and navigation to detail.
- List view with server-backed search, status/creator filters, updated sorting,
  status/ownership columns, and the same navigation.
- Minimal title+description creation flow that opens the created Requirement.
- HTTP reads plus the shared authenticated SSE notification endpoint. This
  change owns the base `GET /events` mechanism and initial
  `requirement.changed` notification; clarification extends the same producer
  with its additional categories. Board/list owns consumption and canonical
  refetch behavior, not browser state reconstruction.
- Frontend test foundation and one grouping/query interaction test.

## Collection scale decision for North 0.1.0

Board and list consume the current single-instance collection returned by
`GET /requirements`. There is no current cursor, page-size, total-count, or
virtualization contract, so this change makes no page-size guarantee and does
not add client pagination, cursor semantics, or virtualization. Collection
scale hardening is a separate prerequisite/change if the single-instance
bounded product scope stops being sufficient.

## Shared browser SSE ownership and invalidation path

This change establishes one authenticated browser SSE endpoint, `GET /events`,
with `requirement.changed` as its initial notification category. The producer
emits a lightweight hint only after the canonical server transaction commits.
The complete board path is:

```text
north-server canonical commit
-> lightweight authenticated GET /events notification
-> board/list refetch GET /requirements
-> render returned canonical Requirement rows
```

The endpoint is notification-only, non-authoritative, non-durable, and not a
WebSocket or replay log. `Last-Event-ID` is not required for correctness;
missed, duplicate, delayed, or out-of-order hints are harmless because HTTP
refetch wins. `introduce-agent-requirement-clarification` extends this same
producer with clarification categories after its canonical transactions. It
does not create another endpoint or event store.

## UI scope

Keep board by lifecycle state, list search/filter/sort, minimal creation,
detail navigation, and frontend tests. Do not add labels, attachments,
advanced prioritization, drag-and-drop lifecycle mutation, or unrelated admin
functionality.

## Capabilities

### New Capabilities

- `requirement-board-ui`: requester board/list rendering, query controls,
  creation, navigation, and notification-driven refetch.

### Modified Capabilities

None. Requirement HTTP semantics already exist.

## Impact and dependencies

- Established prerequisites: requirement domain/concurrency and role contracts.
- This change owns the base authenticated `/events` producer and
  `requirement.changed`; board/list does not depend on clarification or local
  repository inspection.
- Clarification extends this endpoint with clarification categories;
  conversation/detail UI consumes the shared endpoint plus clarification reads.
- `introduce-runtime-retry-and-failure-state` is not required for board/list.

Dependency graph:

```text
introduce-requirement-board
  └─ base GET /events + requirement.changed

introduce-local-repository-inspection
  └─> introduce-agent-requirement-clarification
       └─ extends Board's shared /events categories

introduce-requirement-board + introduce-agent-requirement-clarification
  └─> introduce-requirement-conversation-ui
```
