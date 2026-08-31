# Introduce Requirement conversation/detail UX

## Why

The requester's primary workflow is a detail view that shows conversation beside
structured Requirement truth and honest runtime health. It must remain useful
after reconnects and must never expose raw model reasoning or tool telemetry.

## What Changes

- Extend the Board-owned `/requirements/[id]` Requirement detail shell with
  Conversation, Overview, and Activity tabs.
- Conversation uses the persisted conversation API for requester and agent
  messages. The identity-creating `start` operation may create/reuse a
  sequential run and returns the public `run_id` and `start_message_id`.
  Explicit run-scoped dispatch/cancellation use known `run_id`. The public
  `phase` projection determines legal intent: no session starts a run,
  `awaiting_assignment` permits only same-start retry or cancellation,
  `active` permits later dispatch or cancellation but blocks a competing start,
  and `terminal` permits a new start. `status` remains display-only coarse
  health/result. Latest-session reads may guide presentation, but never supply
  an implicit mutation target; SSE only hints that a refetch may be useful and
  transcript contents never select the operation.
- Overview renders canonical Requirement fields, readiness/repository evidence,
  lifecycle status, content revision, and the minimal clarification session
  `phase`, coarse `status`, cancellation intent, and timestamps supplied by the
  backend.
- Structured edits send the actual `expected_state_version` write token and
  render the server's returned status/revision/state_version. A 409 conflict
  refetches and explains that the Requirement changed; it never blindly retries.
- Activity reads the canonical coarse activity endpoint over HTTP and displays
  only intentionally product-visible summaries.
- Refocus/reconnect refetches Requirement, conversation, latest/current
  assessment, coarse activity, and minimal session/runtime status.

## Backend contract consumed

This change extends the Board-owned `/requirements/[id]` Requirement detail
shell; it does not create or take ownership of that route. It consumes existing
`GET /requirements/{id}` and paged conversation reads plus the existing
persistence-only `POST /requirements/{id}/conversation/messages`. It then uses
clarification's explicit authenticated `start`, run-scoped message dispatch,
and run-scoped cancellation mutations, plus its canonical readiness, activity,
and latest-run reads. The public session projection includes `run_id`,
`start_message_id`, `phase`, `status`, `cancel_requested`, and safe timestamps,
so reload can retry an `awaiting_assignment` run using its persisted start
message. The UI uses `phase`, not coarse `status` alone, for legal intent; the
URL's explicit `run_id`, not latest-read recency, determines every later
mutation target. Latest-run reads may guide presentation but MUST NOT supply an
implicit target, and the UI never performs dispatch or cancellation without a
known `run_id`. Existing protocol `session_id` carries the same identity
(`session_id = run_id`). It consumes the Board-owned shared `GET /events`
endpoint and clarification's added categories; it does not interpret daemon
frames, SSE replay, or a future retry state machine as product truth.

## Execution-status boundary

The initial UI may show the minimal clarification session projection:
`phase` is `awaiting_assignment`, `active`, or `terminal`; coarse `status` is
`starting`, `running`, `completed`, or `unavailable`; and `cancel_requested` is
separate intent. The UI uses `phase`, not `status` alone, to decide whether to
retry the same `start_message_id`, dispatch/cancel an explicit `run_id`, or
start a new run. It does not require, render, or define the later
`Idle`/`Running`/`Retrying`/`Failed` retry machine, retry budget, attempt count,
or final execution failure semantics. A later change may extend the status
badge without changing the canonical conversation/Requirement read flow.

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
  persisted agent messages, explicit run-scoped clarification mutations,
  readiness read model, coarse activity, and latest-run status.
- `introduce-requirement-board` owns the shared authenticated `GET /events`
  infrastructure, `requirement.changed`, and the base read-only
  `/requirements/[id]` detail shell; this change extends that shell.
- Clarification extends Board's `/events` categories with conversation,
  readiness, activity, and session hints.
- `introduce-runtime-retry-and-failure-state` is a later UI extension, not an
  initial dependency.

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
