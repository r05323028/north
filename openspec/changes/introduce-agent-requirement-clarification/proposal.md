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

## North 0.1 runtime strategy

North 0.1 intentionally implements one concrete clarification runtime
integration: `PiClarificationAdapter` in `north-daemon`, backed by Pi Agent. Pi
Agent is North's reference clarification runtime adapter and first end-to-end
runtime vertical slice. This is a deliberate slice to validate the complete
clarification architecture behind a stable North-owned seam, not an attempt to
design or ship a multi-provider runtime framework in 0.1.

The vertical slice is:

1. Requirement clarification starts in North.
2. `north-server` assembles canonical Requirement, deterministic conversation,
   and repository context for the run.
3. The daemon receives the existing North protocol command.
4. The daemon's generic North-owned runtime seam invokes `PiClarificationAdapter`.
5. Pi may inspect only repositories already authorized and bound to that run.
6. Pi produces agent-visible responses, coarse product-visible activity, and
   readiness evidence.
7. `PiClarificationAdapter` maps those results into North-neutral runtime facts.
8. `north-server` persists canonical conversation, readiness, and session
   projections.

Claude Code, Codex, OpenCode, and other runtime integrations are out of scope
for this change. North still MUST keep the seam clean enough to replace Pi
without changing North-owned contracts.

### Architectural invariants

These are architectural invariants, not implementation suggestions:

- **Pi Agent is the North 0.1 reference clarification runtime adapter and first
  end-to-end runtime vertical slice.** Pi-specific APIs, lifecycle concepts,
  configuration, event types, and SDK types MUST remain confined to the
  `north-daemon` adapter and MUST NOT become North protocol, domain,
  persistence, server, or browser concepts.
- **The clarification runtime seam SHALL be defined from North's execution
  needs rather than from Pi Agent's API.** Replacing the Pi adapter with
  another compatible runtime MUST NOT require changes to `north-domain`,
  canonical Requirement/conversation/readiness models, or the server-daemon
  wire protocol.

## What Changes

- Server orchestration for sequential clarification runs: select and durably
  pin an eligible daemon, assemble each run's immutable Requirement snapshot,
  bounded conversation excerpt, and enabled repository metadata, then persist
  and dispatch `session.start` through the existing delivery path. At most one
  active run may compete for a Requirement; terminal runs remain historical.
  User-driven Draft → Discussing starts honor `expected_state_version`; `revision`
  remains content identity, not the write precondition.
- Durable requester-message ordering: persist the canonical message first,
  then create a stable `message.send` command using its persisted identity;
  initial-run messages belong in `session.start` context instead of being sent
  twice.
- Explicit authenticated application mutations: the existing conversation
  message operation persists history only; `clarification/start` starts from a
  persisted message and `expected_state_version`; later message dispatch and
  cancellation explicitly create/reuse their durable protocol commands.
- One North 0.1 concrete daemon runtime integration:
  `PiClarificationAdapter` behind North's internal execution seam, with Pi
  Agent as its provider. Pi SDK lifecycle details stay inside the adapter and
  `north-daemon`; `north-domain`, `north-protocol`, and server contracts remain
  Pi-independent.
- Server processing for runtime events that the protocol already carries:
  durable agent-message, coarse-activity, and per-run session projections,
  followed by ACK only after the relevant projection commits.
- Readiness application through the existing typed assessment conversion and
  atomic persistence/domain path. A stale assessment is durable rejection, not
  a Requirement mutation.
- Canonical HTTP read models for persisted messages, latest/current readiness,
  coarse activity, and minimal session/runtime status.
- Clarification categories on the Board-owned authenticated browser SSE
  producer at `GET /events`, emitted after clarification transactions commit.
  SSE is a hint; HTTP remains the source of Requirement, conversation,
  assessment, activity, and session truth. This change does not create the base
  endpoint or another notification store.

## Authenticated HTTP mutation boundary

Application intent uses explicit authenticated mutations; the existing conversation
message endpoint remains persistence-only:

```text
POST /requirements/{id}/conversation/messages
  -> durable requester message + message_id; no runtime effect
POST /requirements/{id}/clarification/start
  { message_id, expected_state_version }
  -> create/reuse run; start context; no duplicate message.send
POST /requirements/{id}/clarification/messages/{message_id}/dispatch
  -> create/reuse one durable message.send
POST /requirements/{id}/clarification/cancel
-> persist cancel_requested; assigned run creates/reuses session.cancel; unassigned run creates no daemon command
```

`clarification/start` validates the persisted start message and state-version
precondition before any run or command mutation. A valid start with no eligible
daemon still returns its unassigned unavailable run. A reusable unavailable
attempt is retried only with the same recorded `start_message_id`; a terminal or
otherwise inapplicable latest run plus a new eligible message creates a new
sequential run. Later dispatch never creates another conversation message.
Cancellation targets the latest run and never changes Requirement lifecycle or
content; an unassigned cancellation persists `cancel_requested` only and creates
no daemon command or command identity, while an assigned run gets the durable
`session.cancel` command. `GET
/requirements/{id}/session` means latest clarification run: it returns
`{ "session": null }` only before any run exists, and returns unassigned,
assigned/offline, completed, or `cancel_requested` projections otherwise;
status remains within the minimal starting/running/completed/unavailable set. No
generic command API is introduced.

## Minimal execution scope

This change supports sequential clarification runs while allowing at most one
active/competing run per Requirement. Each run has its own requirement/run
binding, immutable snapshot, start message, nullable daemon owner until
assignment, repository set, durable commands/events, cancellation intent, and
coarse starting/running/completed/unavailable projection. A start retry reuses
the latest run only when `daemon_id = null`, no `session.start` was created or
dispatched, it is not cancelled, it is the same logical start attempt, and the
request uses its recorded `start_message_id`. With no eligible daemon, that run
returns `status=unavailable`; after assignment, its daemon pin and run context
are immutable. If the latest run is terminal/inapplicable and a new eligible
persisted message is explicitly started, the server creates a new run with the
current Requirement snapshot; the prior run remains immutable history.
This does
**not** introduce the later execution retry state machine, attempt accounting,
retry budget, server backoff policy, or terminal execution `Failed` decision.
`session.resume` remains an existing execution-recovery command; this change
does not decide when to issue it.

No eligible daemon or an unavailable runtime is operational unavailability. It
must not mark the Requirement failed or invent a business transition; any
explicit, valid Draft → Discussing transition requested by the user remains
canonical.

## Out of scope

No wire-protocol redesign, multi-runtime plugin registry, Claude Code, Codex,
OpenCode, or other runtime adapters; no user-facing provider selection;
no daemon-owned business retry, live daemon migration, raw model reasoning, raw
tool output, coding or source mutation, PR creation, or new
credential/provenance subsystem.

## Capabilities

### New Capabilities

- `clarification-runtime`: server orchestration, per-run runtime invocation,
sequential run lifecycle, event projections, canonical read models, and
clarification notification extensions.

### Modified Capabilities

- `conversations`: agent events become persisted canonical conversation
  messages; requester dispatch follows durable-first ordering.
- `readiness`: assessments produced by the runtime use existing revision/domain
  gates and atomic event ACK semantics.

## Impact and dependencies

- Upstream: configured repository catalog, local repository inspection, merged
  server↔daemon protocol, requirement/conversation/readiness domain contracts,
  Board-owned base `/events`, and durable delivery/session ownership.
- Downstream: conversation/detail UI consumes this change's HTTP read models
  and clarification SSE categories. Board/list rendering and its
  `requirement.changed` invalidation remain independently usable without this
  change.
- The later `introduce-runtime-retry-and-failure-state` change may extend the
  session read model and UI but is not an initial UI prerequisite.

Dependency graph:

```text
introduce-requirement-board
  └─ base GET /events + requirement.changed

introduce-local-repository-inspection
  └─> introduce-agent-requirement-clarification
       └─ extends shared /events categories

introduce-requirement-board + introduce-agent-requirement-clarification
  └─> introduce-requirement-conversation-ui
```
