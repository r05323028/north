# Design

## Decisions

- Review packet endpoint already exists (readiness change); UI renders it plus
  Accept / Request Changes / Reject actions gated client-side by role and
  authoritatively by server guards.
- Stale-guard: page records loaded revision; before any review action, if
  current revision differs, block with explicit stale warning and refetch.
- Request Changes requires a feedback text; stored with the transition audit.
- Reopen lives on Rejected requirements' detail view.

## Open Questions

None.
