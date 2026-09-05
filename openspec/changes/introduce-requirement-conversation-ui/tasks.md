# Tasks

> **Status: Superseded.** This planning artifact is superseded by `introduce-requirement-conversation-workspace`. Do not implement this artifact or merge its contract/task set; use the workspace change as the sole canonical successor.
> **Historical tasks — MUST NOT be executed.** Checkboxes below are retained for context only; implement the successor task set instead.

## 1. Route and canonical reads

- [ ] 1.1 Extend the Board-owned `/requirements/[id]` Requirement detail shell with Conversation, Overview, and Activity tabs; do not create a second route or replace the Board shell.
- [ ] 1.2 Add API client/load state for Requirement, existing paged conversation, clarification readiness, coarse activity, and the public session projection (`run_id`, `start_message_id`, `phase`, `status`, `cancel_requested`, safe timestamps).
- [ ] 1.3 Render structured Requirement fields, lifecycle status, content revision, current assessment/repository citations, and public session `phase`/`status`/cancellation intent from HTTP responses only; never use daemon details.

## 2. Conversation

- [ ] 2.1 Render persisted requester/agent/system messages; keep `POST /requirements/{requirement_id}/conversation/messages` persistence-only and preserve returned message identity.
- [ ] 2.2 Use latest `/session` only to guide presentation; use `phase` and `cancel_requested` to allow no-run `start`, awaiting-assignment same-message retry/cancel, active-run dispatch only when `cancel_requested=false`, active cancellation-pending idempotent cancel without dispatch or another start while the sequential clarification slot is occupied, or terminal-run new `start`; retain returned/public `run_id` and `start_message_id`, require known `run_id` for every later dispatch/cancel, and always send `expected_state_version` on start while relying on server arbitration to apply it only to a genuinely new logical start.
- [ ] 2.3 For a later persisted message, call `POST /requirements/{requirement_id}/clarification/runs/{run_id}/messages/{message_id}/dispatch` only for known assigned `phase=active`, `cancel_requested=false` `run_id`; prove run binding, repeated dispatch reuse, no second message, no status-only action inference, no dispatch during cancellation-pending state, and no retargeting to a newer run.
- [ ] 2.4 Call `POST /requirements/{requirement_id}/clarification/runs/{run_id}/cancel` with known `run_id`; render separate `cancel_requested` intent, keep assigned `phase=active` through `command_ack`, reject later dispatch while cancellation is pending, render unassigned immediate `phase=terminal`, and never substitute latest-run identity.
- [ ] 2.5 Preserve persisted messages and refetch the detail bundle on a stale genuinely new-start HTTP 409 or operational unavailability; render a matching concurrent same-message result as canonical run reuse rather than stale failure; use phase/status to distinguish awaiting-assignment retry, sequential clarification slot retention, and terminal new-start eligibility; if cancellation wins after message persistence, preserve the message and do not dispatch it; never retry with a newer state version or invent a local run.
- [ ] 2.6 Add reconnect/refocus refetch for conversation and prove agent messages survive missed SSE hints.
- [ ] 2.7 Add UI coverage for reload retry using canonical `start_message_id`, Draft/state_version same-message start resolution without stale-new-start failure, phase-driven unavailable handling, concurrent same-message start resolution, concurrent different-message conflict with loser-message history preservation, awaiting retry versus different-message conflict, assigned cancellation after `command_ack` without new run, terminal runtime release, sequential run creation, and explicit run identity preservation.

## 3. Overview and concurrency

- [ ] 3.1 Add inline structured edits with `expected_state_version` in every PATCH request; display `revision` but never use it as the write precondition.
- [ ] 3.2 Handle HTTP 409 by refetching the complete detail bundle and surfacing that the Requirement changed; do not blindly retry.
- [ ] 3.3 Render the exact server response after Ready edits, including Discussing demotion and returned revision/state_version; surface terminal/no-op behavior correctly.

## 4. Activity and minimal status

- [ ] 4.1 Add Activity tab backed by canonical HTTP coarse-activity reads; SSE only triggers refetch and raw diagnostics never render.
- [ ] 4.2 Add public latest-run `phase` (`awaiting_assignment`, `active`, `terminal`), coarse starting/running/completed/unavailable status, separate `cancel_requested`, and safe timestamps; phase drives legal actions, with no run-history UI or later retry budget/attempt/backoff/failure policy.
- [ ] 4.3 Show readiness outcome/current flag and repository ID/full SHA citations without exposing checkout paths or runtime internals.

## 5. Reconnect and privacy tests

- [ ] 5.1 Refetch Requirement, conversation, readiness, activity, and the complete session projection (`run_id`, `start_message_id`, `phase`, `status`, `cancel_requested`, timestamps) after initial load, focus/refocus, reload, SSE disconnect, reconnect, and relevant hints.
- [ ] 5.2 Add fault-injection coverage proving missing transcript does not alter Overview and missed activity hints recover through HTTP.
- [ ] 5.3 Add snapshot/structural checks for no chain-of-thought, raw tool/runtime diagnostics, credentials, checkout paths, or browser WebSocket.
- [ ] 5.4 Add E2E coverage for duplicate/delayed hints, durable requester post, agent reply recovery, reload-safe awaiting-assignment retry, phase-driven active/terminal actions, explicit run-scoped dispatch/cancel URLs, cancellation-pending dispatch rejection and persisted-message race behavior, cancellation intent versus terminal completion, stale run A versus newer run B isolation, and conflict refetch behavior.

## 6. Validation

- [ ] 6.1 Run existing web lint, typecheck, build, and focused frontend/E2E checks.
- [ ] 6.2 Run relevant architecture checks and `openspec validate --all --strict`.
