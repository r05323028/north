# Design

## Context

The critical invariant: Ready valid only for the exact assessed revision,
enforced by domain code that already exists (`mark_ready`, `apply_edit`).

## Decisions

- Assessments stored immutably keyed by (requirement_id, revision, sequence);
  "latest" = highest sequence for current revision.
- `requirement.assessed` events carry the full assessment payload; server
  re-validates every gate via domain before persisting/transitioning — daemon
  verdicts are claims, never commits.
- Review packet is a read-time projection of the current Requirement plus the
  latest valid assessment for exactly that revision: Goal/Scope/Criteria/Open
  Questions come from Requirement; Blockers/assessment assumptions/repos from
  Assessment. No separate packet storage exists in 0.1.0; stale pairs refuse
  projection.
- repositories_reviewed references repository ids from introduce-configured-
  repositories loosely (by id string) so history survives catalog changes.

## Open Questions

None blocking; SHA capture format lands with repository-inspection change.
