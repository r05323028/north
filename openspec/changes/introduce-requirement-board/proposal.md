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
- HTTP reads plus the shared authenticated SSE notification endpoint. The
  server-side invalidation source is explicitly owned by
  `introduce-agent-requirement-clarification`; this change owns board/list
  consumption and canonical refetch behavior, not a second producer.
- Frontend test foundation and one grouping/query interaction test.

## Collection scale decision for North 0.1.0

Board and list consume the current single-instance collection returned by
`GET /requirements`. There is no current cursor, page-size, total-count, or
virtualization contract, so this change makes no page-size guarantee and does
not add client pagination, cursor semantics, or virtualization. Collection
scale hardening is a separate prerequisite/change if the single-instance
bounded product scope stops being sufficient.

## SSE ownership and invalidation path

The complete path is:

```text
north-server canonical commit
        -> lightweight authenticated SSE notification
        -> board/list refetch GET /requirements
        -> render returned canonical Requirement rows
```

SSE notifications are not a browser event log, Requirement truth, WebSocket,
or replay source. Missed or duplicate hints are harmless because HTTP refetch
wins. `introduce-agent-requirement-clarification` owns the common server
producer/endpoint and categories; board/list must consume that contract and
must not reconstruct state from notification payloads.

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
- Shared backend prerequisite: clarification-runtime's canonical SSE producer
  and endpoint; base board/list HTTP rendering remains independently
  implementable from current APIs.
- Downstream/adjacent: conversation/detail UI may use the same notification
  endpoint and canonical Requirement API, but does not depend on board code.
- `introduce-runtime-retry-and-failure-state` is not required for board/list.

Dependency graph:

```text
introduce-local-repository-inspection
                |
                v
introduce-agent-requirement-clarification
          |                   |
          v                   v
introduce-requirement-board   introduce-requirement-conversation-ui
```
