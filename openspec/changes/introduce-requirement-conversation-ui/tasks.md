# Tasks

## 1. Route and canonical reads

- [ ] 1.1 Add deep-linkable `/requirements/[id]` shell with Conversation, Overview, and Activity tabs.
- [ ] 1.2 Add API client/load state for Requirement, existing paged conversation, clarification readiness, coarse activity, and minimal session/runtime reads.
- [ ] 1.3 Render structured Requirement fields, lifecycle status, content revision, current assessment/repository citations, and minimal session status from HTTP responses only.

## 2. Conversation

- [ ] 2.1 Render persisted requester/agent/system messages and post through the canonical message endpoint; preserve returned message identity and durable-first semantics.
- [ ] 2.2 Distinguish initial start-context message from later `message.send` messages through the clarification orchestration contract; never submit the initial message twice.
- [ ] 2.3 Add reconnect/refocus refetch for conversation and prove agent messages survive missed SSE hints.

## 3. Overview and concurrency

- [ ] 3.1 Add inline structured edits with `expected_state_version` in every PATCH request; display `revision` but never use it as the write precondition.
- [ ] 3.2 Handle HTTP 409 by refetching the complete detail bundle and surfacing that the Requirement changed; do not blindly retry.
- [ ] 3.3 Render the exact server response after Ready edits, including Discussing demotion and returned revision/state_version; surface terminal/no-op behavior correctly.

## 4. Activity and minimal status

- [ ] 4.1 Add Activity tab backed by canonical HTTP coarse-activity reads; SSE only triggers refetch and raw diagnostics never render.
- [ ] 4.2 Add minimal starting/running/completed/unavailable status and cancellation intent; do not add retry budget, attempt, backoff, or final execution-failure UI.
- [ ] 4.3 Show readiness outcome/current flag and repository ID/full SHA citations without exposing checkout paths or runtime internals.

## 5. Reconnect and privacy tests

- [ ] 5.1 Refetch Requirement, conversation, readiness, activity, and session status after initial load, focus/refocus, reload, SSE disconnect, reconnect, and relevant hints.
- [ ] 5.2 Add fault-injection coverage proving missing transcript does not alter Overview and missed activity hints recover through HTTP.
- [ ] 5.3 Add snapshot/structural checks for no chain-of-thought, raw tool/runtime diagnostics, credentials, checkout paths, or browser WebSocket.
- [ ] 5.4 Add E2E coverage for duplicate/delayed hints, durable requester post, agent reply recovery, and conflict refetch behavior.

## 6. Validation

- [ ] 6.1 Run existing web lint, typecheck, build, and focused frontend/E2E checks.
- [ ] 6.2 Run relevant architecture checks and `openspec validate --all --strict`.
