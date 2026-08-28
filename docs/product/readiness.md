# Readiness

`Ready` is an **agent verdict**: “clear enough to hand off to a human reviewer.”
It is not acceptance, not a technical design, not an implementation plan.

## Hard gates (enforced in domain code)

An assessment can move a requirement to Ready only if:

- the assessment targets the requirement's **current content revision**;
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

## Two version identities (critical invariant)

```text
assessment.requirement_revision == requirement.revision

While a Requirement is Ready and reviewable:
accepted_assessment.accepted_state_version == requirement.state_version
```

`revision` identifies canonical structured Requirement content. It changes only
when structured content changes and is the value stored in each readiness
assessment's `requirement_revision` binding.

`state_version` identifies mutable Requirement state. It increments once for
every real persisted mutation, including lifecycle transitions, successful Ready
promotion, content edits, and Ready demotion. It does not change on no-op edits,
rejected assessments, or duplicate events. Existing-Requirement HTTP mutations
use `expected_state_version`; stale callers receive HTTP 409 with no side effects.

`accepted_state_version` is assigned once when an accepted assessment promotes a
Discussing Requirement to Ready. It identifies that Ready generation; it is not
a mutable pointer and the historical assessment never changes. Human review
transitions subsequently advance the Requirement token without changing the
evidence, for example `Ready(state_version=6) -> Accepted(state_version=7)`
retains `accepted_state_version = 6`. Exact-generation equality is required when
constructing a review packet or validating Accept, Reject, or Request Changes,
not as a universal invariant for terminal or later lifecycle states.

## Concurrent API and event handling

A `requirement.assessed` event is identity-validated, deduplicated,
sequence-checked, bound to its session Requirement, revision-checked,
domain-validated, and persisted with immutable evidence and any successful
state-version transition in one transaction. Event identity or sequence
conflicts are protocol errors with no assessment ACK; well-formed stale or
invalid assessments receive `event_ack(status=rejected)` only after durable
rejection evidence commits. Successful effects receive
`event_ack(status=accepted)` only after commit. Assessment mutation is a server
service/daemon-event path; browser users only read the review packet.

## Review packet = projection, not source

The human review packet is derived from TWO sources at read time:

```text
ReviewPacket := project(current Requirement, latest valid ReadinessAssessment)
├── from Requirement : goal/title, scope/description, summary,
│                     acceptance criteria, assumptions, open questions,
│                     revision, and state_version
└── from Assessment  : assessment_id, blockers, assessment assumptions,
                      repositories reviewed (+ inspected commit SHAs),
                      requirement_revision
```

The structured Requirement remains the source of truth for content; the
assessment is immutable evidence about exactly one content revision and Ready
state generation. Accepted evidence stores the post-promotion
`accepted_state_version`, and packet lookup requires it to equal the current
Requirement state version. The packet exposes `assessment_id`,
`requirement_revision`, and `requirement_state_version`. Accept, Reject, and
Request Changes must submit the reviewed assessment identity and current state
version; a replaced or stale packet is structurally unreviewable. Conversation
stays supporting context.

See `crates/north-domain/src/{requirement,readiness}.rs`.
