# Tasks

## 1. Foundation

- [ ] 1.1 Add the authenticated web API client for existing Requirement create/list/detail routes; preserve server query names and response version fields.
- [ ] 1.2 Add the minimal SSE consumer for the shared `/events` endpoint; document that the server producer/categories belong to clarification-runtime and that notifications trigger refetch only.
- [ ] 1.3 Add the existing web test setup plus grouping/query-control tests.

## 2. Board and list

- [ ] 2.1 Build fixed Draft/Discussing/Ready/Accepted/Rejected columns and compact cards with title, requester, status, updated time, create action, and detail navigation.
- [ ] 2.2 Build list search, status, creator, and updated sort controls mapped to `GET /requirements`; do not add `limit`, cursor, offset, page-size, or virtualization behavior.
- [ ] 2.3 Build title+description creation flow using the returned Requirement and navigate to `/requirements/[id]`.
- [ ] 2.4 Wire initial load, focus/refocus, SSE notification, and EventSource reconnect to canonical `GET /requirements` refetch; never patch lifecycle from SSE payloads.

## 3. Integration and scope guards

- [ ] 3.1 Verify the full invalidation path: canonical server commit → lightweight shared SSE hint → HTTP collection refetch → rendered server status.
- [ ] 3.2 Add E2E coverage for missed, duplicate, delayed, and reconnect hints; prove rows are not duplicated and no stream replay is needed.
- [ ] 3.3 Add structural/frontend checks for no browser WebSocket, no drag/drop lifecycle mutation, and no labels/attachments/admin feature creep.

## 4. Validation

- [ ] 4.1 Run `npm run lint`, typecheck, and build using the repository's existing web scripts.
- [ ] 4.2 Run relevant architecture checks and `openspec validate --all --strict`.
- [ ] 4.3 If collection scale requires pagination later, stop and create a separate explicit prerequisite/change rather than extending this task silently.
