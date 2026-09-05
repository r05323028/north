# Requirement lifecycle

North's primary loop: a requester creates a requirement, an agent clarifies it
(and may inspect configured source repositories), the structured requirement
evolves, the agent performs a readiness assessment, humans review.

## Business states

```text
Draft ──▶ Discussing ──▶ Ready ──▶ Accepted
                            │            (human decision)
        Request Changes ────┤
                            ▼
                        Discussing   Rejected ──Reopen──▶ Discussing
                                     (human decision)
```

| State | Meaning |
| --- | --- |
| Draft | created, clarification has not meaningfully started |
| Discussing | agent/requester clarifying; agent may inspect repositories |
| Ready | **agent verdict only**: clear enough for human review |
| Accepted | human decision (Requirement Manager/Admin/Owner) |
| Rejected | human decided not to take it; reopenable |

## Version semantics

`revision` is canonical structured-content identity. It changes only when a
real structured edit changes title, description, summary, criteria, assumptions,
or open questions. Readiness evidence always binds to `requirement_revision`.

`state_version` is mutable Requirement-state concurrency. It starts at 1 and
increments once for every real persisted mutation: lifecycle transition,
readiness promotion, content edit, or Ready demotion. No-op edits, rejected
assessments, and duplicate events do not increment either token. Existing-row
HTTP mutations carry `expected_state_version` and stale values return HTTP 409.

Review packets expose both tokens and `assessment_id`; while a Requirement is
Ready and reviewable, `accepted_state_version` must equal its current
`state_version`. Accept, Reject, and Request Changes require that exact Ready
generation and all relevant identity values to still match. Human review then
increments Requirement `state_version` without mutating historical evidence;
for example, `Ready(state_version=6) → Accepted(state_version=7)` retains
`accepted_state_version = 6`. Reopen requires the current state version but no
assessment id.

## Transition ownership

- `Draft → Discussing`: explicit begin-discussion operation starts clarification;
requester messages provide conversation context.
- `Discussing → Ready`: agent readiness assessment verdict, validated by the server.
- `Ready → Accepted`, `Ready → Rejected`, `Ready → Discussing` (Request Changes):
  human reviewers only.
- `Rejected → Discussing` (Reopen): reviewers only.
- Any accepted content edit while `Ready` demotes to `Discussing`
  (see readiness.md). Editing terminal states is refused.

Illegal transitions must be unrepresentable through domain APIs
(`crates/north-domain/src/status.rs`), not just blocked in UI.

## Requester conversation workspace

The direct `/requirements/{id}` route is the requester workspace. It renders
canonical Requirement fields beside durable Conversation messages, independent
readiness, server-published activity, the latest public clarification run, and
current-user identity. Agent questions remain ordinary `agent` messages;
activity is never synthesized into the transcript and completion never implies
Ready.

Requester message submission persists the message first. A new or terminal run
then uses an explicit `clarification/start` with that message ID and current
`state_version`; an active run uses explicit run- and message-scoped dispatch.
An awaiting-assignment or cancellation-pending run occupies its sequential slot:
new dispatch/start is disabled, same-start retry and cancellation remain
explicit, and persisted messages are not rolled back after runtime failure.

The browser uses one `/events` subscription only for category/identity hints.
Reconnect, focus, visibility return, and explicit repair refetch canonical HTTP
state. Structured-content edits use `expected_state_version`; a conflict keeps
the draft and requires reconciliation, while the server response determines
any Ready → Discussing demotion. Reviewer and readiness operations remain
server-authorized and are not requester workspace controls.

## Human review surface

Human review lives in the same `/requirements/[id]` workspace. Ready Requirements
load review truth directly from `GET /requirements/{id}/review-packet`; the
browser does not rebuild packets from conversation/activity or create a second
Requirement/readiness entity. Accept, Reject, and Request Changes send
`assessment_id` plus `expected_state_version`; Reopen sends only
`expected_state_version`. HTTP 409 triggers canonical Requirement/packet
refetch, preserves unsent Request Changes feedback, invalidates the old packet,
and requires explicit reviewer inspection before retry. Requesters may read but
never receive actionable reviewer controls; server authorization remains
authoritative. Current durable review audit rows remain server-owned; this
workspace does not invent a browser history projection.

## Execution state is separate

Runtime execution state (`Idle` / `Running` / `Retrying` / `Failed`) never
mutates business state. A failed agent attempt leaves the Requirement exactly
where it was. The server persists attempts, due retry work, and safe failure
classification; it owns retry/resume/terminal-failure decisions. The daemon
only reconnects, replays, and reports facts. Public clarification projection
keeps `awaiting_assignment` / `active` / `terminal` phases; policy retry is
`active/retrying`, while terminal execution failure is `terminal/failed`. See
docs/architecture/daemon.md and the execution-retry-authority contract.
