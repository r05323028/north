# Introduce execution retry and failure semantics

## Why

Transient daemon/runtime hiccups must not fail work or corrupt Requirements. A
server-owned, bounded retry policy separates infrastructure recovery from
business execution state and survives daemon restarts without split-brain
attempt counts.

## What Changes

- Server-persisted per-session execution state: `Idle` / `Running` /
  `Retrying` / `Failed`, independent of Requirement lifecycle.
- Server-owned attempt count, retry budget, backoff policy, resume decision, and
  terminal failure record. An attempt means a server-directed start/resume, not
  a WebSocket reconnect or replay.
- Daemon owns only `tokio-tungstenite` WebSocket reconnect/backoff, event
  replay, local runtime transport reattachment, and recoverability/failure
  facts. It has no separate business retry budget or permanent-failure
  decision. Axum and tokio-tungstenite provide transport, not reliability.
- Failed ONLY after server budget exhaustion; UI may show failure without
  touching Requirement status or revision.

## Capabilities

### New Capabilities

- `execution-state`: server-authoritative execution state, retry policy,
  restart-persistent attempts, terminal failure, and lifecycle isolation.

### Modified Capabilities

(none)

## Impact

- Affected docs: docs/product/requirement-lifecycle.md (execution-state
  separation note), docs/architecture/daemon.md (retry posture), and
  `harden-distributed-system-architecture`.
- Dependencies on earlier changes: introduce-agent-requirement-clarification.
