## 1. Storage

- [ ] 1.1 Migration 0005: readiness_assessments (immutable rows, revision-bound, repos_reviewed JSON incl. commit SHAs)

## 2. Ingest + validate

- [ ] 2.1 Handler accepting assessment payloads (from protocol events later; direct service call now): run domain mark_ready gates server-side
- [ ] 2.2 Refusals map to explicit errors (stale/blockers/no-criteria/verdict) without state change
- [ ] 2.3 E2E test: edit-after-ready then old-assessment replay refused; fresh assessment promotes

## 3. Packet

- [ ] 3.1 GET review-packet endpoint projecting goal/scope/criteria/assumptions/blockers/repos-inspected
- [ ] 3.2 Packet test on Ready fixture matches all six sections

## 4. Validation

- [ ] 4.1 Full Rust gate
- [ ] 4.2 openspec validate --strict
