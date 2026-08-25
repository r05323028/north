## 1. Storage

- [ ] 1.1 Migration 0003: requirements table (structured columns incl. status, revision, created_by, timestamps) + transition_audit table
- [ ] 1.2 north-persistence mappings; transactional transition wrapper calling domain methods

## 2. API

- [ ] 2.1 Create/list/get endpoints; server-side search/filter/sort params
- [ ] 2.2 Transition endpoints (begin-discussion, accept, reject, request-changes, reopen) with reviewer guards; NO ready endpoint
- [ ] 2.3 Edit endpoint delegating to domain apply_edit (revision bump + demotion)
- [ ] 2.4 Audit rows written on every successful transition

## 3. Tests

- [ ] 3.1 Integration: illegal transitions refused atomically; edit-demotion e2e; terminal edit refused; audit completeness
- [ ] 3.2 Permission tests via roles capability guards

## 4. Validation

- [ ] 4.1 Full Rust gate
- [ ] 4.2 openspec validate --strict
