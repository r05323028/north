## 1. Mechanics

- [ ] 1.1 expires_at on ephemeral tables (migration); retention/cadence config keys
- [ ] 1.2 Batched GC job via persistence interface; scheduler wiring
- [ ] 1.3 Durable-table firewall test (GC SQL touches only ephemeral set)

## 2. Proofs

- [ ] 2.1 Amnesia test: full purge ⇒ requirement/board/packet outputs identical
- [ ] 2.2 Docs update (persistence.md retention mechanics)

## 3. Validation

- [ ] 3.1 Full Rust gate
- [ ] 3.2 openspec validate --strict
