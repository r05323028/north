# Introduce readiness assessment

## Why

Ready must mean something precise: an agent verdict bound to one requirement
revision, validated server-side, invalidated by any later edit. Without a
dedicated assessment concept, Ready decays into vibes.

## What Changes

- The wire `RequirementAssessed` records requirement_revision, typed verdict,
  blockers[], assumptions[], repositories_reviewed[] (non-empty repository_id
  - commit SHA), and assessed_at is applied by the server/domain conversion.
  Canonical outcomes use `event_ack(status=accepted)` or
  `event_ack(status=rejected)`; no opaque assessment JSON or event-name ACK
  aliases.
- Agent verdicts arrive as `requirement.assessed` events; the server validates
  all hard gates through the domain (`mark_ready`) before any transition —
  Discussing→Ready becomes reachable for the first time.
- `requirement.assessed` ingestion is one transaction: dedupe event, lock/current
  revision check, run domain gates, persist immutable evidence and any valid
  transition, commit, then ACK; stale/invalid facts get durable rejection
  handling without promotion.
- Stale assessments are structurally impossible to promote: revision mismatch
  refuses; edits after Ready demote automatically (already enforced in domain,
  now exercised end-to-end).
- The human review packet is a PROJECTION of the current Requirement plus the
  latest valid ReadinessAssessment for exactly that revision — goal/scope/
  criteria from the Requirement (source of truth), blockers/assumptions/repo
  evidence from the assessment. Projection refuses revision mismatch, so a
  stale packet is never reviewable.

Out of scope: who runs the agent (runtime changes), assessment editing,
multiple concurrent assessments.

## Capabilities

### New Capabilities

- `readiness`: assessment model, revision binding, gate validation, stale
  invalidation, review-packet projection.

### Modified Capabilities

- `requirements`: Discussing→Ready edge becomes live, exclusively through
  validated assessments.

## Impact

- Affected docs: docs/product/readiness.md (canonical);
  docs/development/invariants.md row 4 gains end-to-end enforcement note.
- Dependencies on earlier changes: introduce-requirement-conversations.
