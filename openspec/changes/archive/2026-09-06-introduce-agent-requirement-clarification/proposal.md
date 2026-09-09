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
  metadata, then atomically persist the daemon pin, Requirement/run binding,
  immutable context, and complete `session.start` before dispatch through the
  existing delivery path. That authoritative operation changes a run from
  `phase=awaiting_assignment`, `status=unavailable` to
  `phase=active`, `status=starting` and acquires the sequential clarification slot. North
  selects the excerpt in canonical persisted order and always retains
  `start_message_id`; at most one non-terminal clarification run may occupy
  the sequential clarification slot for a Requirement. Both
  `phase=awaiting_assignment` and `phase=active` occupy it; only
  `phase=terminal` releases it. `session.started` only
  confirms runtime startup, retaining `phase=active` while changing status to
  `running`. User-driven Draft → Discussing starts honor
  `expected_state_version`; `revision` remains content identity, not the
  write precondition.
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

`clarification/start` is the identity-creating exception. Server processes each
request in this order: authenticate/authorize; validate the Requirement and
persisted requester-message binding; enter per-Requirement sequential-slot
arbitration; and inspect the authoritative non-terminal occupant, if any. If an
occupant exists and its `start_message_id` matches the request, the request is a
same logical start retry: it reuses the existing `run_id` and command identities,
creates no run, does not reapply Draft → Discussing, and does not reapply the
original `expected_state_version` as a new-mutation precondition. If the message
differs, the request receives the canonical existing-run sequential-slot conflict;
the persisted message remains history and no Requirement mutation, run, or
`session.start` is created. Only an unoccupied slot represents a genuinely new
logical start. The server then validates `expected_state_version` as the atomic
precondition for that new run and any associated Requirement transition. A stale
token returns `409` with no Requirement mutation, run, daemon assignment, or
command, while the already-persisted message remains history. Sequential-slot
arbitration therefore decides reuse, different-message conflict, or new start
before a new-start state-version check. The state version protects creation of a
new logical clarification start and its Requirement transition; it does not
invalidate an already-committed matching logical start during idempotent replay or
concurrent same-message arbitration. A valid start with no eligible daemon still returns its unassigned
`phase=awaiting_assignment`, `status=unavailable` run. A reusable unavailable
attempt is retried only with the same recorded `start_message_id`; a terminal or otherwise inapplicable latest run plus
a new eligible message creates a new sequential run. If a matching
`start_message_id` already has a committed run identity for an unclosed start
attempt, concurrent or retried same-message starts resolve to that run and its
existing command identities:
one serialized request may complete conditional assignment for an awaiting run,
but no request creates a second run or performs a second assignment,
lifecycle transition, or `session.start`. The state-version precondition gates a new start mutation; it
does not turn a matching idempotent retry into a second mutation.

For the concurrent Draft case, if Requirement R is `Draft` at
`state_version=1` and A and B both call `start(M, expected_state_version=1)`,
the winner applies Draft → Discussing once, commits `state_version=2`, and
creates run A. B resolves as an idempotent reference to A's `run_id`; it does
not perform a second transition or fail as a stale new start. If messages differ
(`M1` versus `M2`), the winner occupies the slot and the loser receives the
canonical existing-run/different-message conflict, even though both originally
supplied the current token.

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
  `phase=active` and holds the sequential clarification slot until `session.completed` or
  `session.failed` is durably projected; while `cancel_requested=true`, later
  message dispatch is rejected; and
- `command_ack` for `session.cancel` means only that the daemon durably recorded
  the command, not that runtime cancellation completed.

A stale client targeting run A after run B becomes latest is evaluated only
against run A and cannot mutate, cancel, or create a command for run B. No
dispatch or cancel request silently resolves the latest run. Later message
dispatch is legal only for an assigned `phase=active` run with
`cancel_requested=false`; cancellation remains idempotent for an active
`cancel_requested=true` run, which keeps the sequential clarification slot but rejects new dispatch. If
a requester message is persisted before cancellation wins a race, the message
remains canonical, while dispatch fails/conflicts without creating
`message.send` and without deleting or rolling back the message.

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

## Sequential slot and concurrent start arbitration

For each Requirement, the server/persistence authority SHALL enforce one derived
sequential clarification slot. At most one non-terminal clarification run may
occupy it: `phase=awaiting_assignment` occupies it without a daemon or runtime
execution, `phase=active` occupies it with an assigned non-terminal run, and
only `phase=terminal` releases it. This does not add a phase or another
persisted state machine.

Concurrent identity-creating `clarification/start` requests for one Requirement
SHALL be arbitrated by one server/persistence-authoritative sequential-slot
decision. Creation, reusable-run selection, conflict, and any daemon assignment
MUST be observationally equivalent to one serialized ordering; browser timing
MUST NOT decide whether two non-terminal runs exist. The low-level locking or
transaction mechanism remains an implementation decision. Daemon assignment is
valid only while the run remains the authoritative non-terminal
`phase=awaiting_assignment` occupant and has not been cancelled or closed. The
assignment operation SHALL atomically recheck that eligibility while persisting
the daemon pin, Requirement/run binding, immutable context, complete
`session.start`, `phase=active`, and `status=starting`; if eligibility is lost,
it fails without persisting a command or reactivating the run.

For an empty slot, concurrent starts with the same eligible `message_id` create
or resolve exactly one run and at most one daemon pin and durable `session.start`
identity; with no daemon they resolve to one `phase=awaiting_assignment` run.
Concurrent starts with different eligible messages have one winner and one
canonical existing-run different-message conflict, leaving one occupied slot. Both persisted
messages remain canonical conversation history; the losing message is not
automatically dispatched or rolled back.

Concurrent retries of an awaiting run with its recorded `start_message_id` reuse
that run and perform serialized daemon selection with at most one assignment and
`session.start`. A different message cannot replace that run. An awaiting-run
start retry racing cancellation is equivalent to either cancellation first
(terminal, no daemon or command) or assignment first (active/starting, then the
existing assigned cancellation contract); it MUST NOT expose a hybrid terminal
assigned run.

## Minimal execution scope

This change supports sequential clarification runs with one derived sequential
clarification slot per Requirement. At most one non-terminal clarification run
may occupy it. Both `phase=awaiting_assignment` and `phase=active` occupy the
slot; only `phase=terminal` releases it. Each run has its own `run_id`,
Requirement binding, immutable snapshot, `start_message_id`, nullable daemon pin
until assignment, repository set, durable commands/events, cancellation intent,
and public `phase`/coarse `status` projection. `phase=awaiting_assignment`
means no daemon is assigned, no `session.start` has executed, and the run
occupies the slot while it may be retried or cancelled; `phase=active` means an
assigned non-terminal run occupies the slot, including an assigned disconnected
run and a run with cancellation requested; `phase=terminal` means an unassigned
cancellation or a projected `session.completed`/`session.failed` fact closed
the run and released the slot. `status` remains `starting`, `running`,
`completed`, or `unavailable` and does not decide slot ownership. The existing protocol `session_id` is the same identity as
`run_id`. A start retry may inspect the latest run only under sequential
create/reuse/idempotency rules; dispatch and cancellation never use latest-run lookup and
require explicit `run_id`. This does **not** introduce the later execution retry
state machine, attempt accounting, retry budget, server backoff policy, or
terminal execution `Failed` decision. `session.resume` remains an existing
execution-recovery command; this change does not decide when to issue it. The
phase/status sequence is explicit: no daemon leaves a new run at
`phase=awaiting_assignment`, `status=unavailable`; atomic daemon assignment
plus durable `session.start` persistence sets `phase=active`,
`status=starting` and acquires the sequential clarification slot;
`session.started` retains active and
sets `status=running`; and only `session.completed` or `session.failed`
sets `phase=terminal`.
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
