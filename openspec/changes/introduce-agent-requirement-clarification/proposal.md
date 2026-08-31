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
  deterministic bounded persisted conversation excerpt, and enabled repository
  metadata, then persist and dispatch `session.start` through the existing
  delivery path. North selects the excerpt in canonical persisted order and
  always retains `start_message_id`; at most one non-terminal `phase=active` run may
  compete for a Requirement; terminal runs remain historical. User-driven Draft → Discussing
  starts honor `expected_state_version`; `revision` remains content identity,
  not the write precondition.
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
  coarse activity, and the public phase/status/cancellation session projection.
- Clarification categories on the Board-owned authenticated browser SSE
  producer at `GET /events`, emitted after clarification transactions commit.
  SSE is a hint; HTTP remains the source of Requirement, conversation,
  assessment, activity, and session truth. This change does not create the base
  endpoint or another notification store.

## Authenticated HTTP mutation boundary

Application intent uses explicit authenticated mutations; the existing conversation
message endpoint remains persistence-only:

```text
POST /requirements/{requirement_id}/conversation/messages
  -> durable requester message + message_id; no runtime effect
POST /requirements/{requirement_id}/clarification/start
  { message_id, expected_state_version }
  -> create/reuse sequential run; return public projection with run_id
POST /requirements/{requirement_id}/clarification/runs/{run_id}/messages/{message_id}/dispatch
  -> create/reuse one durable message.send for this run
POST /requirements/{requirement_id}/clarification/runs/{run_id}/cancel
  -> persist cancel_requested; unassigned no-start run becomes terminal, while
     assigned run creates/reuses session.cancel but remains active until terminal fact
```

`clarification/start` is the identity-creating exception: before a client knows a
run ID, it validates the persisted start message and state-version precondition,
resolves the sequential create/reuse rules, and returns a public run projection
including `run_id`. It may inspect the latest run only for that create/reuse
decision. A valid start with no eligible daemon still returns its unassigned
`phase=awaiting_assignment`, `status=unavailable` run. A reusable unavailable
attempt is retried only with the same recorded `start_message_id`; a terminal or otherwise inapplicable latest run plus
a new eligible message creates a new sequential run.

After a run ID is known, dispatch and cancellation are explicitly run-scoped.
Each request SHALL validate that `run_id` exists, belongs to the Requirement in
the URL, and is eligible for the requested operation. An unknown or
Requirement-mismatched run ID on any explicit run-scoped route returns HTTP
`404` with generic error code `not_found` after normal authorization checks; it
must not disclose a run belonging to another Requirement. Dispatch additionally
validates that the persisted message belongs to that Requirement's canonical
conversation, is a requester message eligible for that run, and is not the
recorded start message. Cancellation persists state for that run:

- an unassigned run with no `session.start` execution becomes `phase=terminal`
  immediately, gets no daemon command or command identity, and is ineligible for
  reuse;
- an assigned active run gets one durable `session.cancel`, remains
  `phase=active` and holds the competing-run slot until `session.completed` or
  `session.failed` is durably projected; and
- `command_ack` for `session.cancel` means only that the daemon durably recorded
  the command, not that runtime cancellation completed.

A stale client targeting run A after run B becomes latest is evaluated only
against run A and cannot mutate, cancel, or create a command for run B. No
dispatch or cancel request silently resolves the latest run.

`GET /requirements/{requirement_id}/session` remains a latest-run read convenience
and returns `{ "session": null }` only before any run exists. Its public run
projection includes `run_id`, `start_message_id`, `phase`, `status`,
`cancel_requested`, `updated_at`, and `last_activity_at` (plus safe established
creation timestamps). `phase` is `awaiting_assignment`, `active`, or `terminal`;
`phase` determines slot ownership, while `status` remains the coarse operational
health/result and may be `unavailable` in any phase. Latest-run reads may guide
UI presentation but MUST NOT determine mutation identity. North uses `run_id` in
application and read contracts; existing protocol `session_id` carries that same
stable identity (`session_id = run_id`). No daemon ID, credential, checkout, or
provider detail is public. No generic command API is introduced.

## Minimal execution scope

This change supports sequential clarification runs while allowing at most one
non-terminal active/competing run per Requirement. Each run has its own `run_id`,
Requirement binding, immutable snapshot, `start_message_id`, nullable daemon pin
until assignment, repository set, durable commands/events, cancellation intent,
and public `phase`/coarse `status` projection. `phase=awaiting_assignment` means
no daemon is assigned, no `session.start` has executed, and the run may be
retried or cancelled; `phase=active` means an assigned non-terminal run still
holds the competing slot, including an assigned disconnected run and a run with
cancellation requested; `phase=terminal` means an unassigned cancellation or a
projected `session.completed`/`session.failed` fact closed the run. `status`
remains `starting`, `running`, `completed`, or `unavailable` and does not decide
slot ownership. The existing protocol `session_id` is the same identity as
`run_id`. A start retry may inspect the latest run only under sequential
create/reuse rules; dispatch and cancellation never use latest-run lookup and
require explicit `run_id`. This does **not** introduce the later execution retry
state machine, attempt accounting, retry budget, server backoff policy, or
terminal execution `Failed` decision. `session.resume` remains an existing
execution-recovery command; this change does not decide when to issue it.
For assigned cancellation, the adapter emits existing `session.completed` only
after confirmed runtime termination, or existing `session.failed` for terminal
cancellation failure; `command_ack` is never cancellation completion and no
`session.cancelled` frame is introduced.

No eligible daemon or an unavailable runtime is operational unavailability. It
must not mark the Requirement failed or invent a business transition; any
explicit, valid Draft → Discussing transition requested by the user remains
canonical.

## Deterministic conversation context

North SHALL select each run's conversation context from canonical persisted
conversation history in persisted order. `session.start` SHALL contain a
deterministic bounded excerpt, and SHALL always include the run's
`start_message_id`. For North 0.1, configured size accounting SHALL use a fixed
message count and/or UTF-8 byte size owned by North. Token-based sizing is
deferred unless a later change defines a canonical provider-independent tokenizer
and tokenizer version as part of this configuration; no Pi tokenizer or
abstraction is introduced here.

When the bound is exceeded, North SHALL retain the newest messages that fit,
remove the oldest retained non-start messages first, retain the start message even
when it would otherwise be removed, and emit the retained messages in canonical
persisted order. Identical canonical persisted state and context configuration
SHALL produce the same excerpt. North SHALL persist the selected excerpt in the
immutable run/start context so command replay and reconstruction of the same run
reuse the same context. Pi or another runtime provider SHALL NOT choose which
canonical messages are supplied using provider-specific relevance logic.

The later runtime receives this North-selected context; it does not select or
rewrite canonical conversation history. Exact Pi SDK/API integration remains
inside `PiClarificationAdapter` as documented by the runtime boundary.

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
  ├─ board/list/create/minimal read-only detail
  └─ base GET /events + requirement.changed

introduce-local-repository-inspection
  └─> introduce-agent-requirement-clarification

introduce-requirement-board + introduce-agent-requirement-clarification
  └─> introduce-requirement-conversation-ui
       extends the existing detail shell
```
