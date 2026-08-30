# Tasks

## 1. Server orchestration and context

- [ ] 1.1 Add one clarification-run service for sequential runs with at most one competing active run per Requirement; reuse only an unassigned same-start attempt before daemon selection, and atomically persist owner, Requirement binding, repository IDs, and `session.start` before dispatch when assigned.
- [ ] 1.2 Assemble immutable Requirement snapshot, bounded/relevant conversation excerpt, and enabled repository metadata through existing server DTO conversion; remove any checkout/credential/domain values from the wire input.
- [ ] 1.3 Define the latest-run session projection (`starting`, `running`, `completed`, `unavailable`) plus separate cancellation intent; return `{ "session": null }` only before any clarification run exists, keep older runs as internal history, and do not add retry state, attempt count, budget, backoff, or final `Failed` policy.
- [ ] 1.4 Ensure any Draft → Discussing start transition uses the caller's `expected_state_version`; stale conflict creates no session command while preserving the already-persisted requester message.

## 2. Requester message and command ordering

- [ ] 2.1 Keep requester persistence first; for the initial message include the persisted identity/content in `session.start` context and do not create a second `message.send` command.
- [ ] 2.2 For later messages create/reuse the durable message-to-command mapping and dispatch/replay the exact `message.send` envelope through existing outbox/journal semantics; expose it through the explicit authenticated dispatch operation.
- [ ] 2.3 Add the explicit authenticated clarification cancellation operation; prove unassigned cancellation persists run state without a daemon command, while assigned cancellation creates/reuses one pinned `session.cancel` command and duplicate delivery cannot invoke runtime twice.

## 3. Daemon runtime boundary

- [ ] 3.1 Refine the existing durable runtime seam to accept stable North operation identity and North-neutral clarification input/output rather than a provider SDK lifecycle.
- [ ] 3.2 Add one concrete SDK-backed adapter inside `north-daemon`; keep SDK dependencies and provider types out of `north-server`, `north-domain`, and `north-protocol`.
- [ ] 3.3 Map adapter output to typed protocol events and filter raw tool output/chain-of-thought before journaling or transmission.

## 4. Server event projections and readiness

- [ ] 4.1 Replace delivery-only rejection for `session.started`, `agent.message`, `agent.activity`, `session.completed`, and `session.failed` with idempotent canonical projections and post-commit accepted ACKs.
- [ ] 4.2 Route `requirement.assessed` through existing digest/identity/session/repository/revision/domain transaction logic; preserve durable rejection ACKs for stale/invalid facts and accepted-state-generation recording.
- [ ] 4.3 Define completion-without-assessment and failure-before-assessment behavior; prove duplicate/replayed assessments/completions/failures cannot repeat business or projection effects.

## 5. Canonical reads and browser invalidation

- [ ] 5.1 Add server read models/endpoints for latest readiness (`current` flag), persisted coarse activity, and minimal session/runtime status; keep existing Requirement/conversation/review-packet reads authoritative.
- [ ] 5.2 Extend the Board-owned authenticated `/events` producer with post-commit `conversation.changed`, `readiness.changed`, `activity.changed`, and `session.changed` categories; keep payloads non-authoritative and non-replay-based and add no endpoint or event store.
- [ ] 5.3 Test clarification category hints by refetching HTTP state rather than applying stream payloads; prove the shared producer has one endpoint and no Last-Event-ID correctness dependency.

## 6. Guards and integration

- [ ] 6.1 Test no eligible daemon/runtime availability creates/reuses an unassigned run, preserves durable messages, and leaves Requirement lifecycle/revision/state_version unchanged except for the explicit valid Draft → Discussing transition.
- [ ] 6.2 Test revision edit during a run makes the old assessment stale and produces durable rejection without a Requirement mutation.
- [ ] 6.3 Test duplicate command/event delivery, reconnect, agent-message persistence, coarse activity persistence, and atomic assessment ACK ordering.
- [ ] 6.4 Run architecture checks proving no browser WebSocket, no SDK dependency in protocol/domain, no daemon business retry authority, and no second SSE/source-of-truth path.

## 7. Validation

- [ ] 7.1 Run targeted Rust/PostgreSQL integration tests and relevant web checks.
- [ ] 7.2 Run full required validation and `openspec validate --all --strict`.

## 8. Explicit HTTP intent boundaries

- [ ] 8.1 Keep `POST /requirements/{id}/conversation/messages` persistence-only; prove it returns a durable `message_id` without runtime lookup, run creation, or daemon command.
- [ ] 8.2 Add authenticated `POST /requirements/{id}/clarification/start` with persisted-message validation, `expected_state_version`, reusable-unassigned versus active-conflict versus terminal-new-run rules, current start-context assembly, idempotent same-message behavior, no duplicate `message.send`, and unassigned no-daemon response.
- [ ] 8.3 Add authenticated later-message dispatch by `message_id`; prove Requirement/conversation/run ownership, one durable message-to-command mapping, and idempotent replay without a second conversation message.
- [ ] 8.4 Add authenticated `POST /requirements/{id}/clarification/cancel`; prove separate unassigned run cancellation state with no `session.cancel` command or command identity, assigned pinned-command idempotency, and no Requirement mutation.
- [ ] 8.5 Add HTTP integration scenarios for stale start conflict, no-daemon run identity, unavailable same-message reuse, active concurrent-start conflict, new run after completion/cancellation, current-snapshot capture, duplicate dispatch, and repeated assigned/unassigned cancellation.

## 9. Sequential run lifecycle

- [ ] 9.1 Prove a completed run followed by a new persisted eligible start message creates a new run/session identity with the current Requirement snapshot and independent repository/command context; preserve the prior run as immutable history.
- [ ] 9.2 Prove an assigned active run rejects a different start message with the canonical conflict and creates no second run or `session.start` command.
- [ ] 9.3 Prove an unassigned unavailable run reuses only the same recorded `start_message_id` and same logical start attempt; a different message conflicts until the attempt is cancelled.
- [ ] 9.4 Prove unassigned cancellation persists `cancel_requested` only, creates no `session.cancel` command or command identity, and allows a later new message to create a new run; prove assigned cancellation reuses one pinned command.
- [ ] 9.5 Prove `GET /requirements/{id}/session` returns latest-only data: null before any run, A until B exists, and B after sequential creation, while cancelled/completed A remains historical persistence.
