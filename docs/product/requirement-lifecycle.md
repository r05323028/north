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

Review packets expose both tokens and `assessment_id`; Accept, Reject, and
Request Changes require all relevant identity values to still match the current
Ready state. Reopen requires the current state version but no assessment id.

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

## Execution state is separate

Runtime health (Idle / Running / Retrying / Failed) never mutates business state.
A failed agent run leaves the requirement exactly where it was. The server
persists execution attempts and owns retry/resume/Failed decisions; the daemon
only reconnects, replays, and reports facts. See docs/architecture/daemon.md.
