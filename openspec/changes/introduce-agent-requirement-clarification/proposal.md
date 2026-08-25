# Introduce agent requirement-clarification runtime

## Why

This is North's reason to exist: an agent that discusses a requirement,
inspects relevant source, and returns structured clarification plus a readiness
verdict — while the server keeps every piece of business authority.

## What Changes

- Session orchestration: server sends session.start with structured
  requirement context, conversation context, and the repository catalog;
  message.send continues threads; session.cancel stops work.
- Daemon invokes the local agent runtime behind an internal boundary (one
  concrete implementation allowed; domain stays SDK-free).
- Agent output flows back as agent.message / agent.activity events;
  conversations persist messages; high-level activity only.
- Session completion delivers the structured readiness assessment via
  requirement.assessed; server validation rules already own promotion.

Out of scope: coding/modification execution, PR creation, multi-runtime
plugin frameworks, exposing raw model reasoning anywhere.

## Capabilities

### New Capabilities

- `clarification-runtime`: session lifecycle, context assembly, event
  streaming, assessment delivery, runtime abstraction boundary.

### Modified Capabilities

- `conversations`: agent messages arrive from live sessions.
- `readiness`: assessments arrive from real runs instead of test fixtures.

## Impact

- Affected docs: docs/architecture/daemon.md (runtime section),
  docs/architecture/server-daemon-protocol.md (usage example).
- Dependencies on earlier changes: introduce-local-repository-inspection,
  introduce-readiness-assessment, introduce-server-daemon-protocol.
