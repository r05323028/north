# Roles and permissions

Four instance roles, highest first: **Owner > Admin > Requirement Manager > Requester**.

## North 0.1.0 collaboration policy

North 0.1.0 uses workspace-wide Requirement visibility and collaboration.
Authenticated users can view and converse on workspace Requirements. There is no
per-Requirement ownership or ACL enforcement; these are instance-role
permissions, not Requirement ownership checks.

| Ability | Requester | Req. Manager | Admin | Owner |
| --- | --- | --- | --- | --- |
| Create Requirements | ✓ | ✓ | ✓ | ✓ |
| View workspace Requirements, status, and outcome | ✓ | ✓ | ✓ | ✓ |
| Converse on workspace Requirements | ✓ | ✓ | ✓ | ✓ |
| Edit non-terminal workspace Requirements | ✓ | ✓ | ✓ | ✓ |
| Begin discussion | ✓ | ✓ | ✓ | ✓ |
| Review Ready Requirements: Accept / Reject / Request Changes / Reopen | – | ✓ | ✓ | ✓ |
| Configure repositories, manage daemon settings, instance settings | – | – | ✓ | ✓ |
| Assign roles to users | – | – | ✓* | ✓ |

Requester abilities are to create, view, converse, edit non-terminal workspace
Requirements, begin discussion, and view status/outcome. Requirement Managers,
Admins, and Owners have those same collaborative abilities and additionally
perform reviewer-only lifecycle actions.

\* Admins may grant everything except Owner; Owner grants anything.

## Bootstrap and integrity rules

- Every normal new account starts as **Requester**.
- The first account on a fresh instance atomically becomes **Owner**
  (concurrency-safe; see docs/architecture/persistence.md).
- A user cannot promote themselves; role changes go through
  `assign_role` rules in `crates/north-domain/src/role.rs` and are enforced again
  server-side at the API boundary.

Permission checks are centrally enforced (domain helpers + server-side checks);
UI hiding is cosmetic, never the security boundary.

## Workspace identity and authorization

The workspace reads `/auth/me` for current ID, email, and instance role. Those
values drive labels only; hiding an affordance never replaces server
authorization. Requester structured-content editing is separate from reviewer
operations: Requesters may edit non-terminal content and converse, but only
Requirement Managers, Admins, and Owners may perform Accept, Reject, Request
Changes, Reopen, or other readiness/review mutations. Review actions render in
the canonical `/requirements/[id]` workspace; the client uses the current
review packet only for UX, while server role, lifecycle, assessment, and state
version checks remain authoritative.
