# Introduce Requirement Board and List

## Why

Requesters need to see work at a glance and find requirements fast. Board and
list are the smallest complete surface for that.

## What Changes

- Next.js Board view: columns per lifecycle state, compact cards, create
  action, navigation to detail.
- List view: search, filters (status, creator), sorting (updated), status and
  ownership columns.
- New-requirement dialog limited to title + description.
- Live updates via SSE; no polling storms, no WebSockets.
- First frontend test pattern established.

Out of scope: labels/tags, priorities beyond column order, attachments,
cross-project everything (single instance).

## Capabilities

### New Capabilities

- `requirement-board-ui`: board/list rendering, query interactions, creation
  flow, live status reflection.

### Modified Capabilities

(none)

## Impact

- Affected docs: docs/development/testing.md (frontend test pattern reference).
- Dependencies on earlier changes: introduce-requirement-domain-model,
  introduce-role-and-permission-model.
