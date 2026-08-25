# Introduce execution retry and failure semantics

## Why

Transient daemon/runtime hiccups must not fail work or corrupt requirements. A
bounded, configurable retry policy separates infrastructure health from
business state cleanly.

## What Changes

- Per-session execution state machine: Idle / Running / Retrying / Failed —
  fully separate from the requirement lifecycle.
- Reconnect handling: bounded retries with exponential backoff; resume where
  safe via session.resume.
- Failed ONLY after the retry budget exhausts; failure records reason +
  attempts. UI may show Fail prominently without touching requirement status.
- Constants configurable and documented.

## Capabilities

### New Capabilities

- `execution-state`: liveness-driven state machine, retry policy, terminal
  failure recording, lifecycle isolation guarantees.

### Modified Capabilities

(none)

## Impact

- Affected docs: docs/product/requirement-lifecycle.md (execution-state
  separation note), docs/architecture/daemon.md (retry posture).
- Dependencies on earlier changes: introduce-agent-requirement-clarification.
