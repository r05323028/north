# Daemon

The daemon runs locally near repositories and the agent runtime. It is an
**execution host**, not a business brain.

## Responsibilities

- Persistent, daemon-initiated connection to the server (WebSocket over TLS).
- Local workspace management (clone/fetch via host `git`; see repository-access.md).
- Agent runtime invocation and capability detection.
- Converting runtime output into protocol events; delivering commands to the runtime.
- Heartbeat/liveness; bounded reconnect with backoff; buffering unacknowledged events.

## Non-responsibilities

- No business rules (`if requirement.status == Ready { … }` must not exist here).
- No direct database access; no north-domain dependency.
- No credential custody beyond the local CLI/daemon login credential.

## Failure posture

Transient disconnects are retried with bounded backoff. An execution becomes
`Failed` only when its retry/recovery policy is exhausted — daemon offline alone
never fails a requirement. Constants stay configurable and documented
(OpenSpec change `introduce-runtime-retry-and-failure-state`).

Setup/login follows the Multica-like CLI flow: see
docs/architecture/server-daemon-protocol.md and change
`introduce-daemon-runtime-connection`.
