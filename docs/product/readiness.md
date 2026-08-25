# Readiness

`Ready` is an **agent verdict**: “clear enough to hand off to a human reviewer.”
It is not acceptance, not a technical design, not an implementation plan.

## Hard gates (enforced in domain code)

An assessment can move a requirement to Ready only if:

- the assessment targets the requirement's **current revision**;
- the verdict is Ready with **no unresolved blockers**;
- meaningful acceptance criteria exist;
- current state allows the transition (Discussing).

## Semantic gates (agent judgment)

Before issuing Ready, the agent should be convinced there is no unresolved issue
that could materially change scope, externally observable behavior, or acceptance
criteria: intent understood, scope explicit, blocking questions answered, no known
contradictions, relevant repositories inspected when needed, remaining assumptions
explicit. Implementation trivia (crate choice, table layout, component structure,
internal naming) does NOT block Ready unless it materially changes product behavior.

## Revision binding (critical invariant)

```text
latest_readiness_assessment.requirement_revision == requirement.revision
```

Any edit to a Ready requirement bumps the revision; the old assessment goes stale
and the requirement demotes Ready → Discussing automatically. The agent must
re-assess before Ready again. This is enforced in domain logic — never left to the
agent's memory.

## Assessment record

`ReadinessAssessment` keeps: requirement_revision, verdict, blockers[],
assumptions[], repositories_reviewed[] (repository_id + inspected commit SHA),
assessed_at. See `crates/north-domain/src/readiness.rs`.
