# Design

## Context

Review of the foundation found real gaps: public mutable domain state,
edge-vs-operation confusion, no-op edit churn, a packet ownership error,
protocol direction ambiguity, an overclaimed read-only guarantee, a fragile
manifest-grep archtest parser, and a ledger that blurred enforced vs planned.
The harness must fix these and prevent recurrence without heavyweight tooling.

## Decisions

- **Encapsulation by privacy**: no setter framework; invariant-bearing
  Requirement fields become private with read-only accessors. The compiler is
  the enforcement.
- **Operation ≠ edge**: each aggregate operation pins its source state;
  the transition table stays as the edge map used by status-level reasoning.
- **No-op detection by canonical comparison**: apply-then-compare on plain
  values; no diff engine, no hashing.
- **Packet = projection**: derived at read time from Requirement + revision-
  matched assessment; never stored as truth; stale pairs unrepresentable.
- **Protocol clarity via frame groups**: control frames vs commands vs events;
  server→daemon ACK closes the replay-buffer trimming gap; `session.resume`
  stays command-only. Axum WebSocket and tokio-tungstenite are transport
  adapters; JSON North frames and reliability semantics stay host/protocol
  owned.
- **Read-only honesty**: disposable checkout + dirty-tree violation detection;
  documented as process-level enforcement, not sandbox guarantees.
- **archtests on cargo metadata**: effective graph across normal/dev/build/
  target kinds; pure parser helper covered by a meta-test; serde_json allowed
  only in north-architecture-tests (enforcer crate).
- **Production crate tree boundary**: `crates/` contains runtime components
  only; structural validation and future non-production harnesses live under
  `tests/`, with architecture tests remaining in the workspace.
- **Chronicle-inspired, simplified**: one validate.sh entrypoint, one pre-push
  script, prek consuming standard .pre-commit-config.yaml (prek has no
  prek.toml format), act against real ci.yml jobs, single stable gate job.
  No Chronicle-specific runtime/release machinery.

## Open Questions

None blocking. Branch-protection application remains an owner action
(documented in docs/development/ci.md).
