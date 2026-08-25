# Design

## Decisions

- Review packet endpoint already exists (readiness change); UI renders it plus
  Accept / Request Changes / Reject actions gated client-side by role and
  authoritatively by server guards.
- Stale-guard: page records loaded revision, but the server remains
  authoritative. Each review action submits `expected_revision` and performs
  the atomic match with the revision-matched packet before writing the decision
  audit row; a mismatch returns HTTP 409 with no decision or transition.
- Request Changes requires a feedback text; stored with the transition audit.
- Reopen lives on Rejected requirements' detail view.

## Open Questions

None.
