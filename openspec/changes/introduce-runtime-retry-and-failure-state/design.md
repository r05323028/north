# Design

## Decisions

- ExecutionState tracked per active session on the server (Idle/Running/
  Retrying/Failed); persisted minimally for UI, never joined into requirement rows.
- Retry policy: bounded attempts with exponential backoff + jitter; constants
  (max_attempts, base_delay_ms, max_delay_ms) in server/daemon config, defaults documented.
- Liveness loss starts the retry clock only when an execution is active;
  resume attempts reuse protocol session.resume.
- Failure records reason + attempt count; UI maps state to badges.

## Open Questions

Exact defaults tuned during implementation against real disconnect profiles.
