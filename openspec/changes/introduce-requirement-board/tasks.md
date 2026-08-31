# Tasks

## 1. Foundation

- [x] 1.1 Add the authenticated web API client for existing Requirement create/list/detail routes; preserve server query names and response version fields, including `created_by`.
- [x] 1.2 Add the shared authenticated `GET /events` producer with post-commit `requirement.changed` hints; keep it lightweight, notification-only, non-durable, and independent of clarification. `Last-Event-ID` is not a correctness dependency.
- [x] 1.3 Add the minimal board/list SSE consumer for `/events`; notifications trigger refetch only and never patch cards.
- [x] 1.4 Add the existing web test setup plus grouping/query-control tests.

## 2. Board and list

- [x] 2.1 Build fixed Draft/Discussing/Ready/Accepted/Rejected columns and compact cards with title, creator (`created_by`), status, updated time, create action, and detail navigation.
- [x] 2.2 Build list search, status, creator, and updated sort controls mapped to `GET /requirements`; do not add `limit`, cursor, offset, page-size, or virtualization behavior.
- [x] 2.3 Build title+description creation flow using the returned Requirement and navigate to the Board-owned `/requirements/[id]` detail shell.
- [x] 2.4 Build the minimal read-only `/requirements/[id]` shell from `GET /requirements/{id}`; render canonical fields only and require no clarification, runtime, activity, readiness interaction, or editing.
- [x] 2.5 Wire initial load, focus/refocus, SSE notification, and EventSource reconnect to canonical `GET /requirements` refetch; never patch lifecycle from SSE payloads.

## 3. Integration and scope guards

- [x] 3.1 Verify the full invalidation path: canonical server commit → lightweight shared SSE hint → HTTP collection refetch → rendered server status.
- [x] 3.2 Add E2E coverage for missed, duplicate, delayed, and reconnect hints; prove rows are not duplicated and no stream replay is needed.
- [x] 3.3 Add structural/frontend checks for no browser WebSocket, no drag/drop lifecycle mutation, and no labels/attachments/admin feature creep.
- [x] 3.4 Prove Board/list/create and the Board-owned `/requirements/[id]` shell ship and remain usable when clarification is unavailable or not shipped.

## 4. Validation

- [x] 4.1 Run `npm run lint`, typecheck, and build using the repository's existing web scripts.
- [x] 4.2 Run relevant architecture checks and `openspec validate --all --strict`.
- [x] 4.3 If collection scale requires pagination later, stop and create a separate explicit prerequisite/change rather than extending this task silently.
