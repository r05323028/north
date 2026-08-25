## 1. Workspaces

- [ ] 1.1 Cache layout per repository id; clone/fetch via host Git with inherited environment; reject disabled repositories before work
- [ ] 1.2 Session/task checkout creation from cache with unique paths; concurrent-session test proves mutable directories never overlap
- [ ] 1.3 Read-class Git allowlist and post-task dirty-tree detection; contaminated checkout is discarded and violation reported

## 2. Reporting

- [ ] 2.1 `rev-parse` full SHA capture; inspection results flow as protocol events with repository id
- [ ] 2.2 Integration fixture: inspect → SHA cited; second run reuses cache; disabled repository cannot start; no credential fields cross server boundary

## 3. Validation

- [ ] 3.1 Full Rust gate + `openspec validate --all --strict`
