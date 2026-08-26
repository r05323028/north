# Readiness

`Ready` is an **agent verdict**: “clear enough to hand off to a human reviewer.”
It is not acceptance, not a technical design, not an implementation plan.

## Hard gates (enforced in domain code)

An assessment can move a requirement to Ready only if:

- the assessment targets the requirement's **current revision**;
- the verdict is Ready with **no unresolved blockers**;
- meaningful acceptance criteria exist;
- current state allows the transition (Discussing only).

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

Any content-changing edit bumps the revision; the old assessment goes stale and
the requirement demotes Ready → Discussing automatically. No-op edits (empty or
same-value) change neither revision nor status and do not invalidate assessments.
The agent must re-assess before Ready again — enforced in domain logic, never
left to memory.

## Concurrent API and event handling

Every mutation of an existing Requirement carries `expected_revision`. A stale
caller receives HTTP `409 Conflict` and no content, lifecycle, audit, or
assessment side effect. A `requirement.assessed` event is deduplicated,
revision-checked, domain-validated, and persisted with evidence and any valid
transition in one transaction. The server sends `event_ack(status=accepted)` or
`event_ack(status=rejected)` only after that transaction commits.

## Review packet = projection, not source

The human review packet is derived from TWO sources at read time:

```text
ReviewPacket := project(current Requirement, latest valid ReadinessAssessment)
├── from Requirement : goal/title, scope/description, summary,
│                     acceptance criteria, assumptions, open questions
└── from Assessment  : blockers, assessment assumptions,
                       repositories reviewed (+ inspected commit SHAs)
```

The structured Requirement remains the source of truth for content; the
assessment is evidence about exactly one revision. Projection refuses revision
mismatch, so a stale packet is structurally unreviewable. Conversation stays
supporting context.

See `crates/north-domain/src/{requirement,readiness}.rs`.
