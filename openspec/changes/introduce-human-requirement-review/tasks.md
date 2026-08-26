## 1. Surface

- [ ] 1.1 Review packet view (six sections) on Ready requirements
- [ ] 1.2 Decision buttons role-gated; Request Changes requires feedback textarea; Reopen on Rejected

## 2. Guards

- [ ] 2.1 Revision-match/`expected_revision` check wired into all four decisions (server authoritative; stale maps to HTTP 409)
- [ ] 2.2 Integration/E2E test: edit-between-load-and-decide refused with 409 and no audit/transition; fresh read succeeds
- [ ] 2.3 Audit rows asserted on every decision path

## 3. Validation

- [ ] 3.1 Full Rust gate
- [ ] 3.2 npm gate + openspec validate --strict
