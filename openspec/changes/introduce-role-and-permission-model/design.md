# Design

## Context

Domain already seeds `Role`, `can_review`, `can_administer`, `assign_role`
with unit tests. This change persists roles and enforces them at boundaries.

## Decisions

- Role column on users with CHECK constraint over the four values.
- Server-side guard layer wraps transition/admin endpoints; guards call domain
  helpers so rules exist in exactly one place.
- Assignment endpoint reuses `assign_role(actor, actor_is_target, new_role)`;
  API maps its errors to distinct status codes.
- No middleware framework; explicit guards keep call sites legible.

## Open Questions

None.
