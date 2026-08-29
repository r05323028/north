# Introduce Requirement conversation/detail UX

## Why

The requester's primary workflow is a detail view that shows conversation beside
structured Requirement truth and honest runtime health. It must remain useful
after reconnects and must never expose raw model reasoning or tool telemetry.

## What Changes

- Deep-linkable `/requirements/[id]` route with Conversation, Overview, and
  Activity tabs.
- Conversation uses the persisted conversation API for requester and agent
  messages. After persistence, canonical latest-run state selects explicit
  `start`, same-message unavailable-start retry, later-message `dispatch`, or a
  new sequential start after a terminal/inapplicable run. SSE only hints that a
  refetch may be useful; transcript contents never select the operation.
- Overview renders canonical Requirement fields, readiness/repository evidence,
  lifecycle status, content revision, and the minimal clarification session
  status supplied by the backend.
- Structured edits send the actual `expected_state_version` write token and
  render the server's returned status/revision/state_version. A 409 conflict
  refetches and explains that the Requirement changed; it never blindly retries.
- Activity reads the canonical coarse activity endpoint over HTTP and displays
  only intentionally product-visible summaries.
- Refocus/reconnect refetches Requirement, conversation, latest/current
  assessment, coarse activity, and minimal session/runtime status.

## Backend contract consumed

This change consumes existing `GET /requirements/{id}` and paged conversation
reads plus the existing persistence-only
`POST /requirements/{id}/conversation/messages`. It then uses clarification's
explicit authenticated `start`, message `dispatch`, and `cancel` mutations,
plus its canonical readiness, activity, and latest-run reads. The latest-run
read selects no-run start, reusable same-message start retry, active-run later
message dispatch, or new start after terminal/inapplicable state. It consumes
the Board-owned shared `GET /events` endpoint and clarification's added
categories; it does not interpret daemon frames, SSE replay, or a future retry
state machine as product truth.

## Execution-status boundary

The initial UI may show `starting`, `running`, `completed`, or `unavailable`
when returned by the minimal clarification session read model. It does not
require, render, or define the later `Idle`/`Running`/`Retrying`/`Failed` retry
machine, retry budget, attempt count, or final execution failure semantics.
A later change may extend the status badge without changing the canonical
conversation/Requirement read flow.

## Scope exclusions

No Files tab, attachments, raw chain-of-thought, raw tool/runtime diagnostics,
advanced execution controls, or new Requirement business transitions.

## Capabilities

### New Capabilities

- `requirement-detail-ui`: canonical conversation, structured overview,
  readiness evidence, coarse activity, minimal runtime status, concurrency-safe
  edits, and reconnect/refetch behavior.

### Modified Capabilities

- `conversations`: consumes the persisted conversation as its primary UI.

## Impact and dependencies

- Upstream: archived conversation/readiness/concurrency contracts and current
  Requirement/conversation HTTP APIs.
- Required backend: `introduce-agent-requirement-clarification` provides
  persisted agent messages, explicit clarification mutations, readiness read
  model, coarse activity, and latest-run status.
- `introduce-requirement-board` provides the shared authenticated `GET /events`
  infrastructure and `requirement.changed`; this detail UI consumes it but does
  not depend on Board rendering code.
- Clarification extends Board's `/events` categories with conversation,
  readiness, activity, and session hints.
- `introduce-runtime-retry-and-failure-state` is a later UI extension, not an
  initial dependency.

Dependency graph:

```text
introduce-requirement-board
  └─ base GET /events + requirement.changed

introduce-local-repository-inspection
  └─> introduce-agent-requirement-clarification
       └─ extends shared /events categories

introduce-requirement-board + introduce-agent-requirement-clarification
  └─> introduce-requirement-conversation-ui
```
