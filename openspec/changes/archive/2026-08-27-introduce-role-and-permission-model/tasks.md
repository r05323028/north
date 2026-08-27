## 1. Role storage

- [x] 1.1 Migration 0002: role default and CHECK constraint (column from bootstrap schema)
- [x] 1.2 Map persisted role ↔ domain enum inside north-persistence

## 2. Enforcement

- [x] 2.1 Guard helpers in north-server wrapping domain checks; current user routes guarded, future review/admin routes consume helpers
- [x] 2.2 Role assignment endpoint enforcing assign_role semantics incl. distinct error mapping

## 3. Surface

- [x] 3.1 Users list + role assignment UI (Admin/Owner visible), shadcn/ui components
- [x] 3.2 Frontend reads current user role for affordances only

## 4. Validation

- [x] 4.1 HTTP-boundary/policy tests: requester blocked from review/admin; self-promotion refused; admin→owner refused; owner grants any
- [x] 4.2 Full Rust gate + openspec validate --strict
