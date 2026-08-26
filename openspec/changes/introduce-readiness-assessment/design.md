# Design

## Context

The critical invariant: Ready valid only for the exact assessed revision,
enforced by domain code that already exists (`mark_ready`, `apply_edit`).

## Decisions

- `requirement.assessed` events carry stable id/sequence. Server handling is
  one transaction: dedupe, lock the current Requirement, validate the event
  revision, run domain gates, persist immutable evidence with accepted/rejected
  result, apply a valid transition, persist the row, commit, then send
  `event_ack(status=accepted)` or `event_ack(status=rejected)`. A duplicate committed event repeats
  only its ACK; a rollback emits no ACK.
- `requirement.assessed` events carry typed wire fields:
  `ReadinessVerdictWire`, blockers, assumptions, and
  `ReviewedRepositoryWire { repository_id, commit_sha }`. `north-server`
  explicitly converts these DTOs to `north-domain::ReadinessAssessment` and
  re-validates every gate via domain before persisting/transitioning — daemon
  verdicts are claims, never commits. `north-protocol` never depends on the
  domain crate.
- Review packet is a read-time projection of the current Requirement plus the
  latest valid assessment for exactly that revision: Goal/Scope/Criteria/Open
  Questions come from Requirement; Blockers/assessment assumptions/repos from
  Assessment. No separate packet storage exists in 0.1.0; stale pairs refuse
  projection.
- repositories_reviewed references repository ids from introduce-configured-
  repositories loosely (by id string) so history survives catalog changes.

## Open Questions

None blocking; SHA capture format lands with repository-inspection change.
