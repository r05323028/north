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

## Transition ownership

- `Draft → Discussing`: requester message / clarification begins.
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
