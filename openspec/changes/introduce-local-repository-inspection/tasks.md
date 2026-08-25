## 1. Workspaces

- [ ] 1.1 Workspace dir layout per repository id; clone-or-fetch logic via host git; env inheritance
- [ ] 1.2 Command allowlist (read-only); unit test proves write ops unreachable

## 2. Reporting

- [ ] 2.1 rev-parse SHA capture; inspection results flow as protocol events
- [ ] 2.2 Integration test with a local fixture repo: inspect → SHA cited; second run reuses clone

## 3. Validation

- [ ] 3.1 Full Rust gate
- [ ] 3.2 openspec validate --strict
