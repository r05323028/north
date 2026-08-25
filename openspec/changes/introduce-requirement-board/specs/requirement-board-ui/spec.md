## Purpose

Gives requesters fast orientation: a board for shape, a list for precision,
and a two-field creation flow — all reflecting server truth live.

## ADDED Requirements

### Requirement: Board groups by lifecycle state

The board SHALL render one column per lifecycle state populated from server
data, compact cards showing title/status/requester/updated, a create action,
and card navigation to the detail view.

#### Scenario: Column placement matches server state

- **WHEN** requirements hold mixed statuses
- **THEN** each card appears under exactly its server-reported status column

### Requirement: List supports search, filter, sort

The list SHALL provide text search, status and creator filters, sorting
(updated default), and the same navigation — served by server-side query
parameters rather than full client scans.

#### Scenario: Filter narrows deterministically

- **WHEN** a user filters by status Ready
- **THEN** only Ready requirements render regardless of page size or order of
operations

### Requirement: Creation is minimal

Creating a requirement SHALL require only title and description; the created
Draft appears immediately without extra wizard steps.

#### Scenario: Two fields to first Draft

- **WHEN** a requester submits title+description
- **THEN** a Draft exists and the UI navigates to it

### Requirement: Browser transport stays HTTP/SSE

Board and list SHALL use HTTP requests plus SSE subscriptions for live
updates. No WebSocket usage SHALL appear in frontend code (structural test
enforced).

#### Scenario: Live status flip

- **WHEN** another user's action changes a visible requirement's status
- **THEN** open board views reflect the change via their SSE subscription
without reload
