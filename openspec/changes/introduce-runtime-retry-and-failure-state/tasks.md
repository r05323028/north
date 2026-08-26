## 1. Server-owned state machine

- [ ] 1.1 Add server/persistence execution state model (`Idle`, `Running`, `Retrying`, `Failed`) with durable attempt count, budget, and failure reason per session
- [ ] 1.2 Add server retry policy configuration with documented defaults; increment attempts only for dispatched start/resume commands
- [ ] 1.3 Wire server decision path: failure fact → Retrying → optional durable `session.resume` → Failed only on exhaustion

## 2. Daemon boundary

- [ ] 2.1 Keep daemon reconnect/backoff, event replay, and local runtime reattachment independent of business retry budget; architecture/source tests remain green
- [ ] 2.2 Map daemon `session.failed` to a fact report and prove daemon restart does not reset server attempts

## 3. Isolation proofs

- [ ] 3.1 Integration matrix proves each execution transition leaves Requirement rows, revisions, assessments, and lifecycle untouched
- [ ] 3.2 UI badge wiring shows execution failure without changing lifecycle state

## 4. Validation

- [ ] 4.1 Run retry/restart integration tests, `./scripts/validate.sh fast`, and `openspec validate --all --strict`
