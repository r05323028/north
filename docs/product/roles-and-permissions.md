# Roles and permissions

Four instance roles, highest first: **Owner > Admin > Requirement Manager > Requester**.

| Ability | Requester | Req. Manager | Admin | Owner |
| --- | --- | --- | --- | --- |
| Sign up / log in, create requirements, converse, edit allowed requirements, view status/outcome | ✓ | ✓ | ✓ | ✓ |
| Review Ready requirements: Accept / Reject / Request Changes / Reopen | – | ✓ | ✓ | ✓ |
| Configure repositories, manage daemon settings, instance settings | – | – | ✓ | ✓ |
| Assign roles to users | – | – | ✓* | ✓ |

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
