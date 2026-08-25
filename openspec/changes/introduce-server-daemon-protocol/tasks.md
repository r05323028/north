## 1. Types

- [ ] 1.1 Envelope struct + Command/Event enums (baseline catalog) with serde tags, uuid ids, schema_version
- [ ] 1.2 Round-trip serialization tests; negative test for unknown-type tolerance

## 2. Semantics

- [ ] 2.1 Server-side dedupe window keyed by message id (unit tests incl. duplicate assessed event)
- [ ] 2.2 Daemon-side JSONL buffer of unacked events; resume handshake replays once, acks trim
- [ ] 2.3 Archtests confirm protocol purity after adding deps

## 3. Validation

- [ ] 3.1 Full Rust gate
- [ ] 3.2 openspec validate --strict
