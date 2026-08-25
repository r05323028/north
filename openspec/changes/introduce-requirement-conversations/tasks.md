## 1. Storage + API

- [ ] 1.1 Migration 0004: conversations, messages (kind CHECK constraint)
- [ ] 1.2 Post message (requester); paginated thread read; structured-state endpoint unchanged/independent
- [ ] 1.3 Structured-edit endpoint via apply_edit returning new revision

## 2. Guarantees

- [ ] 2.1 Test: pruning all messages leaves structured state byte-identical
- [ ] 2.2 Test: edit-from-conversation bumps revision exactly once; Ready demotes

## 3. Validation

- [ ] 3.1 Full Rust gate
- [ ] 3.2 openspec validate --strict
