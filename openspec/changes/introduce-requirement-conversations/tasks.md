## 1. Storage + API

- [ ] 1.1 Migration 0004: conversations, messages (kind CHECK constraint)
- [ ] 1.2 Post message (requester); paginated thread read; structured-state endpoint unchanged/independent
- [ ] 1.3 Structured-edit endpoint via apply_edit requiring `expected_revision`, returning new revision, and mapping stale rows to HTTP 409

## 2. Guarantees

- [ ] 2.1 Test: pruning all messages leaves structured state byte-identical
- [ ] 2.2 Test: edit-from-conversation bumps revision exactly once; Ready demotes; stale expected_revision returns 409 with no message or state change

## 3. Validation

- [ ] 3.1 Full Rust gate
- [ ] 3.2 openspec validate --strict
