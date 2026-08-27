# Design

## Context

Domain already seeds `Role`, `can_review`, `can_administer`, `assign_role`
with unit tests. This change persists roles and enforces them at boundaries.

## Decisions

- Role column on users with CHECK constraint over the four values.
- Server-side guard helpers call domain checks so rules exist in exactly one
  place. Current user-management routes use them; review and configuration
  handlers owned by later changes must consume the same helpers.
- Assignment endpoint reuses `assign_role(actor, actor_is_target, new_role)`;
  API maps its errors to distinct status codes. No future review or
  configuration endpoint is invented in this foundation change.
- No middleware framework; explicit guards keep call sites legible.

## Open Questions

None.
