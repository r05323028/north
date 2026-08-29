# Introduce agent requirement-clarification runtime

## Why

North's core loop is an agent discussing a Requirement, optionally inspecting
configured source, and returning structured clarification plus a readiness
verdict. The server must orchestrate that loop without giving business
authority to a daemon or making a transport frame the product source of truth.

## Existing protocol boundary

The merged `north-protocol` contract already defines typed commands
`session.start`, `session.cancel`, `session.resume`, and `message.send`, plus
`session.started`, `agent.message`, `agent.activity`, `requirement.assessed`,
`session.completed`, and `session.failed` events. It also defines delivery
envelopes, stable IDs, directional sequences, canonical
`command_ack`/`event_ack`, reconciliation, and terminal protocol errors.

This change consumes that contract. It does not add aliases, redesign payloads,
move ACK ownership, or duplicate reconciliation/retry rules.

## What Changes

- Server orchestration for one clarification run: select and durably pin an
  eligible daemon, assemble the immutable requirement snapshot, bounded
  conversation excerpt, and enabled repository metadata, then persist and
  dispatch `session.start` through the existing delivery path. User-driven
  Draft → Discussing starts honor `expected_state_version`; `revision` remains
  content identity, not the write precondition.
- Durable requester-message ordering: persist the canonical message first,
  then create a stable `message.send` command using its persisted identity;
  initial-run messages belong in `session.start` context instead of being sent
  twice.
- One concrete daemon runtime adapter behind North's internal execution seam;
  SDK lifecycle details stay inside the daemon and `north-domain`/
  `north-protocol` remain SDK-independent.
- Server processing for runtime events that the protocol already carries:
  durable agent-message, coarse-activity, and one-run session projections,
  followed by ACK only after the relevant projection commits.
- Readiness application through the existing typed assessment conversion and
  atomic persistence/domain path. A stale assessment is durable rejection, not
  a Requirement mutation.
- Canonical HTTP read models for persisted messages, latest/current readiness,
  coarse activity, and minimal session/runtime status.
- Authenticated browser SSE notification production from post-commit server
  changes. SSE is a hint; HTTP remains the source of Requirement, conversation,
  assessment, activity, and session truth.

## Minimal execution scope

This change supports one clarification execution and its minimal persisted
session facts: requirement/session binding, owner when selected, durable
commands/events, cancellation intent, coarse starting/running/completed/
unavailable projection, and runtime facts. An unavailable start creates or
retains an unassigned coarse run record; it never creates a fake daemon
execution. It does
**not** introduce the later execution
retry state machine, attempt accounting, retry budget, server backoff policy,
or terminal execution `Failed` decision. `session.resume` remains an existing
execution-recovery command; this change does not decide when to issue it.

No eligible daemon or an unavailable runtime is operational unavailability. It
must not mark the Requirement failed or invent a business transition; any
explicit, valid Draft → Discussing transition requested by the user remains
canonical.

## Out of scope

No wire-protocol redesign, multi-runtime plugin registry, daemon-owned business
retry, live daemon migration, raw model reasoning, raw tool output, coding or
source mutation, PR creation, or new credential/provenance subsystem.

## Capabilities

### New Capabilities

- `clarification-runtime`: server orchestration, one-run runtime invocation,
  event projections, canonical read models, and browser invalidation hints.

### Modified Capabilities

- `conversations`: agent events become persisted canonical conversation
  messages; requester dispatch follows durable-first ordering.
- `readiness`: assessments produced by the runtime use existing revision/domain
  gates and atomic event ACK semantics.

## Impact and dependencies

- Upstream: configured repository catalog, local repository inspection, merged
  server↔daemon protocol, requirement/conversation/readiness domain contracts,
  and durable delivery/session ownership.
- Downstream: board/list and conversation/detail UI consume the HTTP read models
  and shared SSE endpoint. Board HTTP rendering may be developed independently,
  but live invalidation depends on this change's server notification producer.
- The later `introduce-runtime-retry-and-failure-state` change may extend the
  session read model and UI but is not an initial UI prerequisite.

Dependency graph:

```text
introduce-local-repository-inspection
                |
                v
introduce-agent-requirement-clarification
          |                   |
          v                   v
introduce-requirement-board   introduce-requirement-conversation-ui
```
