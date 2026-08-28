# Design

## Decisions

- Review packet endpoint already exists (readiness change); UI renders it plus
  Accept / Request Changes / Reject actions gated client-side by role and
  authoritatively by server guards.
- Stale-guard: page records `assessment_id`, content revision, and state version,
  but the server remains authoritative. Each review action submits the assessment
  identity and `expected_state_version`; persistence matches the exact accepted
  Ready-generation evidence before writing decision provenance and the audit row.
  A mismatch returns HTTP 409 with no decision or transition.
- Request Changes requires a feedback text; stored with the transition audit.
- Reopen lives on Rejected requirements' detail view.

## Open Questions

None.
