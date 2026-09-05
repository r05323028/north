# Tasks

## 1. Shared browser contracts and API boundary

- [ ] 1.1 Preserve existing `apps/web/lib/requirements.ts` exports used by Board/List and extract or reuse one authenticated JSON request/error path that retains HTTP status and server `error` code.
- [ ] 1.2 Add strict shared parsers/types for Conversation pages, messages, clarification runs, readiness, activities, and `/auth/me`; accept only current snake_case fields, closed enums, required IDs/timestamps, and positive JavaScript-safe version numbers.
- [ ] 1.3 Add API operations for the existing Requirement, conversation, structured-edit, readiness, activity, session, requester-message, clarification start/explicit dispatch/cancel, current-user, and `/events` contracts; preserve the wire `session` wrapper while exposing one browser `ClarificationRun` concept.
- [ ] 1.4 Add parser/API tests for malformed responses, unknown enum values, HTTP status/code classification, paged responses, 503 `clarification_unavailable`, 409 conflicts, and current-user role strings.
- [ ] 1.5 Validate this slice with `npm test` in `apps/web` and `./scripts/validate.sh web`.

## 2. Canonical workspace load and route

- [ ] 2.1 Replace only the Board-owned `/requirements/[id]` detail body with the new workspace while preserving direct links, refresh, Board/List navigation, and not adding a second route.
- [ ] 2.2 Implement one detail-bundle load state for Requirement, loaded conversation pages, readiness, activity, latest run, current user, loading/refreshing/error state, and actual SSE connection state.
- [ ] 2.3 Load the initial bundle from canonical HTTP responses, keep each response scoped to its own resource, retain successful stale data during non-initial refresh errors, and ignore older bundle responses after newer requests complete.
- [ ] 2.4 Add explicit paged conversation/activity loading using existing `next_offset`; track `prior_loaded_end_offset` as the exclusive end of the largest contiguous history range loaded from offset 0, not a page index. On canonical repair discard cached conversation slices, restart at `offset=0`, let each server-returned `next_offset` control traversal, continue until the rebuilt range reaches that prior end and every prior stable message ID is re-observed (fetching beyond the old numeric end when page positions shift), and continue through the new end when the prior range had reached end. Merge by stable ID and render authoritative `(created_at, id)` order without stream-payload insertion or stale offset reuse.
- [ ] 2.5 Add component tests for direct-route loading, complete bundle rendering, stale-response protection, refresh-error retention, `prior_loaded_end_offset` repair across new-message page-boundary shifts, duplicate overlap/no-duplication, no-omission continuity by stable ID, reconnect/focus repair after history expansion, stale repair-generation suppression, and missing assessment/history.
- [ ] 2.6 Validate this slice with focused Vitest coverage plus `npm run lint` and `npm run typecheck` in `apps/web`.

## 3. Conversation and responsive presentation

- [ ] 3.1 Render requester, agent, and system messages from the shared Message contract; present agent clarification questions as agent messages and coarse activity as status/activity content, never synthetic transcript entries.
- [ ] 3.2 Render Conversation as primary pane and Live Requirement as a separate landmark; use existing shadcn/ui primitives, two visible desktop panes, and stacked Conversation-then-Requirement small-screen flow.
- [ ] 3.3 Keep composer, status announcements, activity, and Requirement access usable at small widths and by keyboard; add accessible names, focus handling, live/status regions, and non-color-only state cues.
- [ ] 3.4 Add component tests for message roles, timestamps, empty/loading/error states, progress summaries, desktop structure, and small-screen DOM accessibility.
- [ ] 3.5 Validate this slice with focused Vitest tests and `./scripts/validate.sh web`.

## 4. Composer intent, run identity, and failure states

- [ ] 4.1 Implement persistence-first submission: retain composer text until requester-message POST succeeds, retain returned `message_id`, and never invoke runtime intent from the persistence operation alone.
- [ ] 4.2 Implement no-run/terminal submission as explicit clarification start with current `state_version`; retain returned `run_id` and `start_message_id`, and never send initial message through `message.send`.
- [ ] 4.3 Implement active `phase` submission as explicit run-scoped dispatch only when `cancel_requested=false`; allow durable concurrent messages to target the same run and never resolve latest run implicitly.
- [ ] 4.4 Implement awaiting-assignment behavior: disable new clarification submissions, preserve unsent draft, expose same-start retry using canonical `start_message_id`, expose cancellation, and handle 503 without automatic retry or second run.
- [ ] 4.5 Implement active cancellation-pending behavior: disable later dispatch/new start, preserve explicit run target, allow idempotent cancellation/refetch, and keep the slot occupied through command acknowledgement.
- [ ] 4.6 Implement terminal completed/unavailable behavior, cancellation-success/failure copy, occupied-slot conflicts, stale-start 409 handling, dispatch/cancel 404 or 409 handling, and saved-but-not-dispatched messaging without deleting history or rerouting to a newer run.
- [ ] 4.7 Add component/API tests for persistence-before-start, initial-message no-duplicate dispatch, active explicit URLs, concurrent same/different message outcomes, reload-safe awaiting retry, cancellation races, terminal new run, and stale run A versus newer run B isolation.
- [ ] 4.8 Validate this slice with focused Vitest coverage and the relevant existing clarification HTTP integration tests.

## 5. Live Requirement, readiness, and current-user affordances

- [ ] 5.1 Render title, description, summary, criteria, assumptions, open questions, lifecycle status, creator, timestamps, revision, and state version only from the canonical Requirement response.
- [ ] 5.2 Render readiness verdict/currentness/blockers/assumptions and retained repository ID/full SHA only from the canonical readiness response; never infer Ready from transcript, activity, or run completion and never expose checkout paths/credentials.
- [ ] 5.3 Keep inline structured editing in this slice because the canonical `conversations` requirement makes conversation-surface edits part of North 0.1; use only the existing PATCH contract with displayed `expected_state_version`, apply the returned Requirement, show canonical Ready-to-Discussing demotion, and refuse optimistic lifecycle/readiness prediction.
- [ ] 5.4 Handle structured-edit 409 by retaining unsaved draft, refetching the complete bundle, and requiring user reconciliation; show terminal edit refusal and other server errors without local bypass. Do not add reviewer, readiness, or restricted-lifecycle controls to this editor.
- [ ] 5.5 Load `/auth/me` through the shared contract, label the current requester from canonical ID/email/role when needed, remove hard-coded person/email/role assumptions from the workspace and shell, and keep reviewer affordances cosmetic only.
- [ ] 5.6 Add tests for canonical-only rendering, readiness currentness, repository citation privacy, structured-content edit response, stale edit reconciliation, terminal refusal, Requester rejection of reviewer/readiness operations, and precise role affordances.
- [ ] 5.7 Validate this slice with focused Vitest coverage, `npm run lint`, and `npm run typecheck` in `apps/web`.

## 6. Realtime invalidation and honest connection state

- [ ] 6.1 Extend the shared EventSource subscription contract without creating another `/events` endpoint; parse named notification identity/category, filter workspace hints by `requirement_id`, and preserve Board/List behavior.
- [ ] 6.2 Expose actual connecting/connected/reconnecting/closed-or-error state for the workspace, remove the shell's hard-coded connected claim, and keep SSE informational rather than canonical.
- [ ] 6.3 Coalesce relevant `requirement.changed`, `conversation.changed`, `readiness.changed`, `activity.changed`, and `session.changed` hints into canonical bundle refetches; use the same path for EventSource reconnect, browser focus, visibility return, and explicit repair.
- [ ] 6.4 Prove malformed, unrelated, duplicate, delayed, reordered, missed, and overrun notifications cannot duplicate messages/activity/runs or patch Requirement state; never open a browser WebSocket or use `Last-Event-ID` for correctness.
- [ ] 6.5 Add unit and Playwright coverage for SSE disconnect/reconnect, focus repair, duplicate/delayed hints, missed agent-message recovery, missed activity recovery, unrelated Requirement filtering, and stale HTTP response suppression.
- [ ] 6.6 Validate this slice with `npm test`, `npm run test:e2e`, and `./scripts/validate.sh web` in the documented environment.

## 7. Server-authority and end-to-end contract proofs

- [ ] 7.1 Add or extend authenticated integration coverage proving workspace-wide view/conversation/cancel/begin-discussion policy, and separately proving the existing non-terminal structured-content edit contract (currently Requester, Requirement Manager, Admin, and Owner) without treating it as generic lifecycle editing; prove Requester review/readiness/restricted-lifecycle attempts remain forbidden and no per-Requirement creator ACL is introduced.
- [ ] 7.2 Verify existing server integration coverage for message persistence without runtime lookup, same-message start reuse, different-message conflict, awaiting/active/terminal phase rules, cancellation command identity, readiness currentness, and no Requirement mutation on runtime failure; add only backward-compatible server projection fixes if a required safe field is genuinely missing.
- [ ] 7.3 Add browser-boundary E2E coverage for direct route load, durable requester post, explicit start/dispatch/cancel URLs, active unavailable state, cancellation pending, terminal completion, failure before assessment, reload retry, and conflict/refetch behavior.
- [ ] 7.4 Add privacy/structural checks proving no chain-of-thought, raw prompts/tool traces, daemon/provider IDs, credentials, checkout paths, command payloads, browser WebSocket, duplicate local canonical entity types, or second SSE source exists in the workspace.
- [ ] 7.5 Validate this slice with the focused server integration tests, `npm run test:e2e`, and relevant architecture tests.

## 8. Compatibility documentation and completion gate

- [ ] 8.1 Perform a compatibility review against `openspec/specs/requirements`, `conversations`, `readiness`, `roles`, `browser-reconnect`, `distributed-delivery`, `daemon-protocol`, `session-ownership`, `execution-retry-authority`, `repository-isolation`, and the active clarification/runtime changes; record any duplicate name, enum, field meaning, or identity mapping decision in the design or implementation notes. Include the predecessor's superseded status.
- [x] 8.2 Mark `introduce-requirement-conversation-ui` superseded in its proposal, design, tasks, and capability spec; use `introduce-requirement-conversation-workspace` as the sole canonical successor and do not execute or merge the predecessor task/spec set.
- [ ] 8.3 Update landed canonical behavior in `docs/product/requirement-lifecycle.md`, `docs/product/roles-and-permissions.md`, `docs/architecture/overview.md`, `docs/architecture/persistence.md`, `docs/development/testing.md`, and proven rows in `docs/development/invariants.md`; do not mark unexecuted E2E or smoke coverage complete.
- [ ] 8.4 Review the complete diff, run `./scripts/validate.sh fast`, run `openspec validate --all --strict`, and run `./scripts/pre-push-validation.sh` because implementation/source/test changes are present; skip pre-push only if the final Git changed-file union is strictly documentation-only under `AGENTS.md`.
