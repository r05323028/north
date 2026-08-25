# Introduce role and permission model

## Why

Review rights, instance administration, and self-promotion prevention must be
central rules, not scattered checks. Roles gate every later endpoint (review,
repositories, daemons, users).

## What Changes

- Persist one role per user: Owner / Admin / Requirement Manager / Requester.
- Central permission checks completing the foundation-seeded domain helpers
  (`Role::can_review`, `Role::can_administer`, `assign_role`), enforced again
  at every API boundary.
- Role assignment API: Owner grants any role; Admin grants everything except
  Owner; nobody modifies their own role; only Owner/Admin may assign.
- Admin-only surfaces (repositories, daemon settings, instance settings, user
  management) reject non-admin actors server-side.
- UI hides/disables actions per role but is never the security boundary.

Out of scope: complex enterprise RBAC, multiple simultaneous roles per user,
per-repository permissions, invitation systems.

## Capabilities

### New Capabilities

- `roles`: role persistence, permission gates, assignment rules.

### Modified Capabilities

- `email-auth`: account creation now stamps the standard role (Requester;
  first-owner rule unchanged and still atomic).

## Impact

- Affected docs: docs/product/roles-and-permissions.md (canonical),
  docs/development/invariants.md (ledger rows 5, 11).
- Domain crate gains no new dependencies; checks reuse seeded pure functions.
- Dependencies on earlier changes: introduce-email-auth-and-owner-bootstrap.
