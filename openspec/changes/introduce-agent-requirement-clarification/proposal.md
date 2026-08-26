# Introduce agent requirement-clarification runtime

## Why

This is North's reason to exist: an agent that discusses a requirement,
inspects relevant source, and returns structured clarification plus a readiness
verdict — while the server keeps every piece of business authority and pins the
run to one selected daemon.

## What Changes

- Session orchestration: server selects and persists `session.daemon_id`, then
  sends durable `session.start` with typed requirement fields, a bounded/relevant
  conversation excerpt, and enabled repository metadata only; `message.send`,
  `cancel`, and execution-only `resume` use the durable command contract with
  stable ids. Credentials and domain types never cross the wire.
- Daemon invokes the local agent runtime behind an internal boundary (one
  concrete implementation allowed; domain stays SDK-free).
- Agent output flows back as `agent.message` / `agent.activity` events with
  directional sequence; conversations persist messages and high-level
  activity only.
- Session completion delivers `requirement.assessed`; the server's atomic
  revision/dedupe/domain transaction owns promotion, evidence, and event ACK.

Out of scope: coding/modification execution, PR creation, multi-runtime plugin
frameworks, exposing raw model reasoning, daemon migration, and a daemon-owned
business retry budget.

## Capabilities

### New Capabilities

- `clarification-runtime`: session lifecycle, pinned routing, context assembly,
  event streaming, assessment delivery, and runtime abstraction boundary.

### Modified Capabilities

- `conversations`: agent messages arrive from live sessions.
- `readiness`: assessments arrive from real runs instead of test fixtures.

## Impact

- Affected docs: docs/architecture/daemon.md and
  docs/architecture/server-daemon-protocol.md.
- Cross-cutting contracts: `harden-distributed-system-architecture`.
- Dependencies on earlier changes: introduce-local-repository-inspection,
  introduce-readiness-assessment, introduce-server-daemon-protocol.
