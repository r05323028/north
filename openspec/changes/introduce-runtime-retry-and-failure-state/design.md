# Design

## Context

Execution health is not Requirement lifecycle. The daemon can lose transport
or runtime state, but only the server can decide whether another execution
attempt is warranted. Cross-cutting ownership is canonical in
`harden-distributed-system-architecture`.

## Decisions

- Persist `ExecutionState`, `attempt_count`, retry budget/policy inputs, and
  last failure reason in server-owned session/execution storage; do not join
  these fields into Requirement rows.
- Server policy uses bounded attempts with exponential backoff and jitter. The
  retry configuration lives in server configuration and has documented
  defaults. `attempt_count` increments only when the server dispatches a new
  `session.start` or `session.resume` execution attempt.
- Axum WebSocket reconnects, North heartbeat retries, frame replay,
  daemon-local `tokio-tungstenite` runtime transport recovery, and event
  reattachment do not increment the business attempt count. The libraries own
  transport mechanics only; the server owns retry decisions. They report facts
  to the server.
- Server transitions `Running → Retrying`, decides whether to send a new
  durable `session.resume`, and transitions to `Failed` only on exhaustion.
  Daemon `session.failed` is a recoverability/failure fact, not authority.
- All execution transitions are persisted independently of Requirement rows.
  A failure never changes Requirement status, revision, assessment, or
  conversation truth.

## Risks / Trade-offs

- **A daemon cannot make progress while its pinned owner is offline** → retain
  the owner and let server retry/failure policy make the visible decision; do
  not silently migrate live sessions.
- **Backoff defaults need operational tuning** → keep them server configuration,
  document initial defaults with the implementation, and test a changed bound.
- **Duplicate resume delivery** → use the durable command id/sequence contract;
  daemon command dedupe prevents a second runtime invocation.
