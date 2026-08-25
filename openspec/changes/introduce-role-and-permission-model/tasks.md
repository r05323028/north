## 1. Role storage

- [ ] 1.1 Migration 0002: role column on users with CHECK constraint; default Requester
- [ ] 1.2 Map persisted role ↔ domain enum inside north-persistence

## 2. Enforcement

- [ ] 2.1 Guard helpers in north-server wrapping domain checks; apply to admin/review endpoints (existing ones guarded)
- [ ] 2.2 Role assignment endpoint enforcing assign_role semantics incl. distinct error mapping

## 3. Surface

- [ ] 3.1 Users list + role assignment UI (Admin/Owner visible), shadcn/ui components
- [ ] 3.2 Frontend reads current user role for affordances only

## 4. Validation

- [ ] 4.1 Integration tests: requester blocked from review/admin; self-promotion refused; admin→owner refused; owner grants any
- [ ] 4.2 Full Rust gate + openspec validate --strict
