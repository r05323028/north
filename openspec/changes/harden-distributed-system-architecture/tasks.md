## 1. Canonical contracts

- [x] 1.1 Add the seven capability specs under `specs/` for delivery, Requirement concurrency, session ownership, retry authority, repository isolation, browser reconnect, and architecture guardrails.
- [x] 1.2 Review protocol acknowledgement terminology, sequence rules, stale-event handling, daemon ownership, and retry authority against the design; remove contradictory wording before implementation begins.

## 2. Architecture and persistence documentation

- [x] 2.1 Update `docs/architecture/server-daemon-protocol.md` with durable command outbox/inbox, `command_ack`, directional sequence spaces, reconciliation, duplicate/gap behavior, and 0.1.x compatibility.
- [x] 2.2 Update `docs/architecture/{overview,daemon,persistence,repository-access}.md` with session pinning, server retry authority, disposable checkout isolation, `disabled_at`, credential ownership, and durable/ephemeral classes.
- [x] 2.3 Update `docs/architecture/dependency-boundaries.md` and `docs/development/testing.md` with structural limits and the later integration/E2E proof plan.
- [x] 2.4 Update `docs/development/invariants.md` so every new guarantee is honestly `Specified` or `Partially Enforced` until a running mechanism exists.

## 3. Mechanical enforcement available now

- [x] 3.1 Extend `tests/architecture/tests/architecture.rs` to reject daemon ownership of server execution retry state while allowing transport backoff/replay.
- [x] 3.2 Keep protocol purity, dependency direction, repository-validation layout, and frontend WebSocket checks green; add credential-schema checks only when server-side repository schemas exist.

## 4. Align pending implementation changes

- [x] 4.1 Amend `introduce-server-daemon-protocol` with command outbox/inbox ACKs, directional sequences, gap reconciliation, compatibility errors, and daemon-ledger compaction tests.
- [x] 4.2 Amend `introduce-daemon-runtime-connection` and `introduce-agent-requirement-clarification` with durable `daemon_id` pinning, credential ownership/revocation, and same-daemon reconnect routing.
- [x] 4.3 Amend `introduce-runtime-retry-and-failure-state` so server persists attempt policy/state and daemon owns only transport/local recovery mechanics.
- [x] 4.4 Amend `introduce-configured-repositories` and `introduce-local-repository-inspection` with `disabled_at`, history-preserving identity, session/task checkout isolation, and concurrency/dirty-tree tests.
- [x] 4.5 Amend Requirement, conversation, readiness, and human-review changes with mandatory `expected_revision`, HTTP 409 conflicts, and atomic `requirement.assessed` transaction/ACK tests.
- [x] 4.6 Amend board/detail UI changes with SSE reconnect/refetch semantics and no replay-derived Requirement truth.

## 5. Validation and consistency

- [x] 5.1 Run architecture tests and targeted domain tests after mechanical changes.
- [x] 5.2 Search docs and OpenSpec artifacts for stale claims about hard deletion, shared workspaces, daemon retry ownership, legacy ACK names, opaque assessment payloads, resume event cursors, or SSE replay.
- [x] 5.3 Run `openspec validate --all --strict` and the repository standard validation profiles; record runtime suites that are not yet executable rather than claiming them passed.
