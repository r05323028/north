## 1. Storage

- [ ] 1.1 Migration 0005: readiness_assessments (immutable rows, revision-bound, repos_reviewed JSON incl. commit SHAs)

## 2. Ingest + validate

- [ ] 2.1 Handler accepting typed `requirement.assessed` payloads (from protocol events later; direct service call now): convert wire evidence to domain assessment, dedupe event, lock/current revision, run domain mark_ready gates, persist evidence/transition atomically, and send `event_ack(status=...)` only after commit
- [ ] 2.2 Refusals map to explicit errors/rejection ACKs (stale/blockers/no-criteria/verdict) without Requirement state change
- [ ] 2.3 Integration: duplicate and stale `requirement.assessed` events produce one effect or durable rejection, ACK only after commit, and never partial evidence/state

## 3. Packet

- [ ] 3.1 GET review-packet endpoint projecting goal/scope/criteria/assumptions/blockers/repos-inspected
- [ ] 3.2 Packet test on Ready fixture matches all six sections

## 4. Validation

- [ ] 4.1 Full Rust gate
- [ ] 4.2 openspec validate --strict
