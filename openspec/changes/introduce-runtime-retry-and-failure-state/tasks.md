## 1. State machine

- [ ] 1.1 ExecutionState model + per-session tracking; persistence minimal
- [ ] 1.2 Retry policy (bounded, exp backoff+jitter) config keys + documented defaults
- [ ] 1.3 Resume integration with protocol session.resume; failure record (reason, attempts)

## 2. Isolation proofs

- [ ] 2.1 Test matrix: each execution transition leaves requirement rows unchanged
- [ ] 2.2 UI badge wiring (Fail visible, lifecycle untouched)

## 3. Validation

- [ ] 3.1 Full Rust gate
- [ ] 3.2 openspec validate --strict
