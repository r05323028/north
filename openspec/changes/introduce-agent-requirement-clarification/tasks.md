# Tasks

## 1. Server orchestration and context

- [ ] 1.1 Add one clarification-run service that reuses current daemon selection/pinning and durable command APIs; persist owner, Requirement binding, repository IDs, and `session.start` before dispatch.
- [ ] 1.2 Assemble immutable Requirement snapshot, bounded/relevant conversation excerpt, and enabled repository metadata through existing server DTO conversion; remove any checkout/credential/domain values from the wire input.
- [ ] 1.3 Define the coarse one-run session projection (`starting`, `running`, `completed`, `unavailable`) plus cancellation intent without adding retry state, attempt count, budget, backoff, or final `Failed` policy.
- [ ] 1.4 Ensure any Draft → Discussing start transition uses the caller's `expected_state_version`; stale conflict creates no session command while preserving the already-persisted requester message.

## 2. Requester message and command ordering

- [ ] 2.1 Keep requester persistence first; for the initial message include the persisted identity/content in `session.start` context and do not create a second `message.send` command.
- [ ] 2.2 For later messages create/reuse the durable message-to-command mapping and dispatch/replay the exact `message.send` envelope through existing outbox/journal semantics.
- [ ] 2.3 Add idempotent cancellation command handling and prove duplicate/replayed cancellation cannot invoke runtime twice.

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
- [ ] 5.2 Add one authenticated post-commit SSE producer/endpoint with requirement, conversation, readiness, activity, and session notification categories; keep payloads non-authoritative and non-replay-based.
- [ ] 5.3 Test missed, duplicate, and reconnect hints by refetching HTTP state rather than applying stream payloads.

## 6. Guards and integration

- [ ] 6.1 Test no eligible daemon/runtime availability leaves Requirement lifecycle and revision/state_version unchanged while preserving durable messages.
- [ ] 6.2 Test revision edit during a run makes the old assessment stale and produces durable rejection without a Requirement mutation.
- [ ] 6.3 Test duplicate command/event delivery, reconnect, agent-message persistence, coarse activity persistence, and atomic assessment ACK ordering.
- [ ] 6.4 Run architecture checks proving no browser WebSocket, no SDK dependency in protocol/domain, no daemon business retry authority, and no second SSE/source-of-truth path.

## 7. Validation

- [ ] 7.1 Run targeted Rust/PostgreSQL integration tests and relevant web checks.
- [ ] 7.2 Run full required validation and `openspec validate --all --strict`.
