# Harden distributed-system architecture

## Why

North's existing topology is sound, but its protocol only makes daemon→server
replay explicit. Before server, daemon, persistence, and protocol implementation
starts, the remaining command-delivery, ordering, concurrency, ownership,
retry, workspace, history, compatibility, and browser-reconnect contracts must
be settled so reconnects cannot create duplicate effects or stale business
writes.

## What Changes

- Define durable server→daemon command delivery: server outbox, daemon durable
  inbox/processed ledger, at-least-once transport, idempotent command identity,
  explicit accepted/completed boundaries, sequence reconciliation, and safe
  dedupe compaction.
- Define independent per-session server-command and daemon-event sequences,
  gap handling, duplicate/late/out-of-order behavior, and minimal 0.1.x
  protocol-version negotiation/error behavior.
- Make every existing-Requirement mutation revision-aware, return stale
  conflicts as HTTP 409, and define the single transaction/ack boundary for
  `requirement.assessed`.
- Pin each session to a selected daemon identity; define user-owned
  credentials, administrator revocation, no live migration, and server-owned
  execution retry policy/state.
- Isolate every clarification execution in a disposable session/task checkout
  backed by a reusable daemon repository cache; preserve process-level dirty
  tree enforcement and host-only credentials.
- Soft-disable configured repositories instead of deleting identity rows, so
  historical assessment evidence remains readable and disabled repositories
  cannot start new inspections.
- Define SSE as a live notification hint: reconnect/refetch canonical API
  state, never reconstruct Requirement truth from replay.
- Strengthen architecture tests for dependency/transport/retry ownership and
  add implementation-time integration test tasks for behavioral guarantees.

The founding topology remains unchanged: browser↔server uses HTTP+SSE;
server↔daemon uses a daemon-initiated Axum WebSocket ↔ North JSON text ↔
`tokio-tungstenite` path; the server owns business state, and no broker,
external runner, object storage, or kernel sandbox is introduced.

## Capabilities

### New Capabilities

- `distributed-delivery`: durable command delivery, directional sequence
  spaces, reconciliation, and 0.1.x compatibility.
- `requirement-concurrency`: expected-revision mutation conflicts and atomic
  assessment ingestion.
- `session-ownership`: daemon selection, pinning, credential ownership, and
  revocation behavior.
- `execution-retry-authority`: server-owned execution state and retry budget;
  daemon transport recovery remains local mechanics.
- `repository-isolation`: disposable concurrent inspection workspaces and
  durable repository identity/history.
- `browser-reconnect`: SSE notification and canonical-state refetch semantics.
- `architecture-guardrails`: structural enforcement and deferred behavioral
  test obligations.

### Modified Capabilities

No main OpenSpec specs exist yet. Existing pending change specs are updated to
reference these canonical contracts rather than duplicating them.

## Impact

- Canonical docs: `docs/architecture/{overview,daemon,persistence,
  repository-access,server-daemon-protocol,dependency-boundaries}.md` and
  `docs/development/{invariants,testing}.md`.
- Pending OpenSpec contracts for protocol, daemon connection, retry,
  repositories, inspection, readiness, requirement mutations, clarification,
  and browser UI are aligned to this change.
- `tests/architecture` gains only structural checks possible before runtime
  implementation; durable delivery, concurrency, ownership, and SSE behavior
  remain integration/E2E tasks for their owning implementation changes.
- No production server/daemon/persistence stack is implemented here.
