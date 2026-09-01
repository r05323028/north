# Introduce Requirement Board and List

## Why

Users need a fast view of Requirement work and a precise way to find it.
Board and list are the smallest user-facing surface for that workflow.

## Existing backend capability

The current `north-server` Requirement API already provides the backend this UI
needs:

- `POST /requirements` creates a Draft from title and description;
- `GET /requirements/{id}` retrieves a complete Requirement;
- `GET /requirements` supports server-side `search`, `status`, `created_by`,
  and updated-time sorting; and
- responses include canonical lowercase lifecycle status identifiers
  (`draft`, `discussing`, `ready`, `accepted`, `rejected`), `revision`, and
  `state_version`.

The frontend keeps those identifiers as its wire/domain values and maps them to
human-readable title-case labels only for presentation. This change does not
reimplement, paginate, or redesign those APIs.

## What Changes

- Next.js Board view with one column per lifecycle state, compact cards, create
action, and navigation to the Board-owned detail shell.
- List view with server-backed search, status/creator filters, updated sorting,
creator columns, and the same navigation.
- Minimal read-only Requirement detail shell at `/requirements/[id]`, using the
existing `GET /requirements/{id}` response and no clarification dependency.
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

Keep board by lifecycle state, list search/filter/sort, minimal creation, the
minimal read-only Requirement detail shell, and frontend tests. Do not add
clarification, runtime status, activity, readiness interaction, editing,
labels, attachments, advanced prioritization, drag-and-drop lifecycle mutation,
or unrelated admin functionality.

## Capabilities

### New Capabilities

- `requirement-board-ui`: board/list rendering, query controls, creation,
  minimal read-only detail shell, navigation, and notification-driven refetch.

### Modified Capabilities

None. Requirement HTTP semantics already exist.

## Impact and dependencies

- Established prerequisites: requirement domain/concurrency and role contracts.
- This change owns the Board/list/create surface, the minimal read-only
  `/requirements/[id]` detail route/shell, the base authenticated `/events`
  producer, and `requirement.changed`; it does not depend on clarification or
  local repository inspection.
- Clarification extends the shared endpoint with clarification categories;
  conversation/detail UI extends this change's existing detail shell and
  consumes clarification reads.
- `introduce-runtime-retry-and-failure-state` is not required for board/list or
  the base detail shell.

Dependency graph:

```text
introduce-requirement-board
  ├─ board/list/create/minimal read-only detail
  └─ base GET /events + requirement.changed

introduce-local-repository-inspection
  └─> introduce-agent-requirement-clarification

introduce-requirement-board + introduce-agent-requirement-clarification
  └─> introduce-requirement-conversation-ui
       extends the existing detail shell
```
