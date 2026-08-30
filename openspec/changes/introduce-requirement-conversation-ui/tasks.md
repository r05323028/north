# Tasks

## 1. Route and canonical reads

- [ ] 1.1 Extend the Board-owned `/requirements/[id]` Requirement detail shell with Conversation, Overview, and Activity tabs; do not create a second route or replace the Board shell.
- [ ] 1.2 Add API client/load state for Requirement, existing paged conversation, clarification readiness, coarse activity, minimal session/runtime reads, and public `run_id` handling.
- [ ] 1.3 Render structured Requirement fields, lifecycle status, content revision, current assessment/repository citations, and minimal session status from HTTP responses only.

## 2. Conversation

- [ ] 2.1 Render persisted requester/agent/system messages; keep `POST /requirements/{requirement_id}/conversation/messages` persistence-only and preserve returned message identity.
- [ ] 2.2 Use latest `/session` only to guide presentation; invoke identity-creating `start` for no-run/same-message retry/new sequential start and retain returned `run_id`; require a known `run_id` for every later dispatch/cancel, always send `expected_state_version` on start, and never submit the start message as `message.send`.
- [ ] 2.3 For a later persisted message, call `POST /requirements/{requirement_id}/clarification/runs/{run_id}/messages/{message_id}/dispatch` with known `run_id`; prove run binding, repeated dispatch reuse, no second message, and no retargeting to a newer run.
- [ ] 2.4 Call `POST /requirements/{requirement_id}/clarification/runs/{run_id}/cancel` with known `run_id`; validate/render returned operational status without changing Requirement content/lifecycle or substituting latest-run identity.
- [ ] 2.5 Preserve persisted messages and refetch the detail bundle on start HTTP 409 or operational unavailability; never retry with a newer state version or invent a local run.
- [ ] 2.6 Add reconnect/refocus refetch for conversation and prove agent messages survive missed SSE hints.
- [ ] 2.7 Add UI coverage for sequential run creation after completion/cancellation, active-run concurrent-start conflict, same-message unavailable-start reuse, and preserving returned `run_id`.

## 3. Overview and concurrency

- [ ] 3.1 Add inline structured edits with `expected_state_version` in every PATCH request; display `revision` but never use it as the write precondition.
- [ ] 3.2 Handle HTTP 409 by refetching the complete detail bundle and surfacing that the Requirement changed; do not blindly retry.
- [ ] 3.3 Render the exact server response after Ready edits, including Discussing demotion and returned revision/state_version; surface terminal/no-op behavior correctly.

## 4. Activity and minimal status

- [ ] 4.1 Add Activity tab backed by canonical HTTP coarse-activity reads; SSE only triggers refetch and raw diagnostics never render.
- [ ] 4.2 Add minimal latest-run starting/running/completed/unavailable status and separate `cancel_requested`; do not add run-history UI, retry budget, attempt, backoff, or final execution-failure UI.
- [ ] 4.3 Show readiness outcome/current flag and repository ID/full SHA citations without exposing checkout paths or runtime internals.

## 5. Reconnect and privacy tests

- [ ] 5.1 Refetch Requirement, conversation, readiness, activity, and session status after initial load, focus/refocus, reload, SSE disconnect, reconnect, and relevant hints.
- [ ] 5.2 Add fault-injection coverage proving missing transcript does not alter Overview and missed activity hints recover through HTTP.
- [ ] 5.3 Add snapshot/structural checks for no chain-of-thought, raw tool/runtime diagnostics, credentials, checkout paths, or browser WebSocket.
- [ ] 5.4 Add E2E coverage for duplicate/delayed hints, durable requester post, agent reply recovery, sequential start selection, explicit run-scoped dispatch/cancel URLs, stale run A versus newer run B isolation, and conflict refetch behavior.

## 6. Validation

- [ ] 6.1 Run existing web lint, typecheck, build, and focused frontend/E2E checks.
- [ ] 6.2 Run relevant architecture checks and `openspec validate --all --strict`.
