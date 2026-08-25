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
- Review packet endpoint derives Goal/Scope/Criteria/Assumptions/Blockers/
  Repos from the latest valid assessment + requirement fields; no separate
  packet storage in 0.1.0.
- repositories_reviewed references repository ids from introduce-configured-
  repositories loosely (by id string) so history survives catalog changes.

## Open Questions

None blocking; SHA capture format lands with repository-inspection change.
