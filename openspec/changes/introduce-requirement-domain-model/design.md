# Design

## Context

Domain logic is already seeded and unit-tested in north-domain. This change
wraps it in durable storage + API without duplicating rules.

## Decisions

- Persistence stores the structured fields plus current status/revision;
  transitions go through domain methods inside a transaction so illegal states
  are unwritable even under races.
- Transition audit rows record actor, from/to, timestamp (minimal auditability).
- Agent-driven entry into Ready is deliberately NOT implemented here — it
  arrives with introduce-readiness-assessment; until then no endpoint can set
  Ready, keeping the assessment contract authoritative.
- List endpoint supports server-side search/filter/sort primitives reused by
  both board and list UI later.

## Open Questions

None.
