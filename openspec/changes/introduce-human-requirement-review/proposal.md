# Integrate human Requirement review into canonical workspace

## Why

`main` already owns review decisions, reviewer authorization, optimistic
concurrency, Ready-generation identity, and durable transition audit rows. A
separate review page would duplicate those contracts and create a second
Requirement/readiness truth. This change makes the existing
Requirement Conversation Workspace the browser surface for those contracts.

## What changes

- Extend only `/requirements/[id]` with review presentation and actions.
- Load review truth from `GET /requirements/{id}/review-packet`; never rebuild a
  packet from messages, activity, or client readiness heuristics.
- Show Accept, Reject, and Request Changes for eligible reviewers while the
  packet is current and the Requirement is Ready. Show Reopen for rejected
  Requirements.
- Send the exact existing mutation preconditions: `assessment_id` plus
  `expected_state_version` for Accept, Reject, and Request Changes; only
  `expected_state_version` for Reopen.
- Treat HTTP 409 as stale repair: refetch Requirement and packet, preserve
  unsent Request Changes feedback, and require an explicit accessible refreshed-
  state acknowledgement before another mutation (`Review refreshed packet` for
  Ready decisions, or `Review refreshed Requirement` for Reopen).
- Keep Requesters read-only for review actions. Client visibility is UX only;
  server role checks remain authoritative.
- Keep durable audit writes server-owned. This change does not add a history
  endpoint or label coarse workspace activity as audit history.

## Capabilities

### New Capabilities

- `human-review`: browser review presentation, exact mutation payloads, and
  stale packet repair.

### Modified Capabilities

- `requirement-conversation-workspace`: renders review in the existing
  Requirement route and consumes safe canonical review state.

The server-side `requirements`, `readiness`, and `roles` contracts are
consumed, not redesigned.

## Non-goals

No new review route, review persistence model, lifecycle enum, readiness model,
ACL model, browser WebSocket, optimistic lifecycle transition, automatic retry,
or duplicate Requirement/readiness entity.

## Dependencies

Consumes current `requirements`, `readiness`, `roles`, review-packet,
transition, and Requirement Conversation Workspace contracts. Completed changes
are treated as current behavior, not future prerequisites.

## Documentation impact

Update lifecycle, role, architecture, testing, and invariant documentation to
name the single workspace surface, exact mutation identities, stale repair, and
absence of a browser audit-history projection.
