# Introduce readiness assessment

## Why

Ready must mean something precise: an agent verdict bound to one requirement
revision, validated server-side, invalidated by any later edit. Without a
dedicated assessment concept, Ready decays into vibes.

## What Changes

- ReadinessAssessment records: requirement_revision, verdict, blockers[],
  assumptions[], repositories_reviewed[] (repository_id + commit SHA),
  assessed_at.
- Agent verdicts arrive as `requirement.assessed` events; the server validates
  all hard gates through the domain (`mark_ready`) before any transition —
  Discussing→Ready becomes reachable for the first time.
- Stale assessments are structurally impossible to promote: revision mismatch
  refuses; edits after Ready demote automatically (already enforced in domain,
  now exercised end-to-end).
- Latest-valid-assessment payload powers the human review packet:
  Goal/Scope/Criteria/Assumptions/Blocking Questions/Repositories Inspected.

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
