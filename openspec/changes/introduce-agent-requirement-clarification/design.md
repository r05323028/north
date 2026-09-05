# Design

## Existing implementation consumed

This change completes behavior around existing seams instead of redefining
 them:

- `north-protocol` is the sole wire-type catalog and already validates the
  typed command/event families and envelope identities.
- `north-server::assemble_session_start` converts server snapshots into the
  complete requirement/conversation/repository context.
- `AuthStore::start_session_with_command_for_requirement_and_repositories`
  selects/pins a daemon, validates active repository IDs, persists the complete
  command envelope, and retains the run repository set.
- `DaemonRuntime::persist_and_dispatch_command` dispatches the persisted
  representation only through the pinned daemon.
- `AuthStore::post_requester_message` commits requester history before it
  returns a message identity.
- `AuthStore::record_readiness_assessment_with_event_digest` owns assessment
  identity/sequence checks, session binding, repository identity/run checks,
  revision gates, immutable evidence, and the atomic Requirement promotion.

The current server deliberately durably rejects generic runtime events with
`event_handler_not_implemented`. This change replaces that downstream handling
for the clarification projections; it does not change the wire contract.

## Ownership matrix

| Concern | Owner in this slice |
| --- | --- |
| Frame names, payloads, envelope IDs/sequences, ACK/reconciliation | `north-protocol` and existing delivery coordination |
| Session selection/pinning and command outbox | `north-server`/`north-persistence` |
| Requirement lifecycle, revision, state version, readiness gates | `north-domain` through server persistence |
| Repository selection and run-bound inspection | server context + `introduce-local-repository-inspection` |
| Pi Agent invocation and provider mapping | `PiClarificationAdapter` in `north-daemon`, behind the North-owned seam |
| Agent messages/activity/session projections and HTTP reads | `north-server`/`north-persistence` |
| Base browser SSE endpoint and `requirement.changed` | `introduce-requirement-board` |
| Clarification SSE categories and post-commit emission | `north-server` in this change, extending Board's `/events` |
| Clarification HTTP mutations | `north-server`/`north-persistence` |
| Browser rendering/refetch | board and conversation UI changes |
| Retry budget, attempts, server backoff, final execution failure | later `introduce-runtime-retry-and-failure-state` |

## Authenticated clarification mutations

These are explicit authenticated application operations, not a generic command
API. The existing conversation persistence boundary remains independent from
runtime availability. All routes use the existing Requirement/conversation
authorization rules.

- `POST /requirements/{requirement_id}/conversation/messages` accepts
  `{ "body": string }`, commits one requester message, and returns `201 Created`
  with the persisted message and `message_id`. It never transitions the
  Requirement, creates a run, selects a daemon, or creates `session.start` or
  `message.send`. Runtime availability cannot prevent this history write.
- `POST /requirements/{requirement_id}/clarification/start` accepts
  `{ "message_id": string, "expected_state_version": u64 }`. It validates that
  the persisted requester message belongs to this Requirement's conversation
  and is eligible as the run's start message. A daemon-backed start returns
  `202 Accepted` with the canonical Requirement and public run projection,
  including `run_id` and `start_message_id`. A valid start with no eligible
  daemon returns `503 Service Unavailable` with error code
  `clarification_unavailable`, the canonical Requirement, and the unassigned
  `phase=awaiting_assignment`, `status=unavailable` run projection. After slot
  arbitration identifies an unoccupied slot as a genuinely new start, a stale
  state version returns the canonical `409` conflict before any new-run or
  command mutation; the message remains persisted. A matching non-terminal
  start is idempotent and does not reapply its original state-version token.
- `POST /requirements/{requirement_id}/clarification/runs/{run_id}/messages/{message_id}/dispatch`
  has no message body. It validates that `run_id` exists, belongs to the
  Requirement in the URL, is `phase=active` with `cancel_requested=false` and an assigned,
  non-terminal run (including a pinned daemon that is operationally unavailable),
  and that the
  persisted requester message belongs to this Requirement's canonical
  conversation, is eligible for that run, and is not the recorded start message.
  It returns `202 Accepted` after creating or reusing exactly one durable
  `message.send` mapping. An `awaiting_assignment` or terminal run cannot receive
  dispatch. A pinned offline daemon keeps the command durable and reports
  operational unavailability. It never creates another conversation message or
  resolves a newer latest run.
- `POST /requirements/{requirement_id}/clarification/runs/{run_id}/cancel` has no
  message body. It validates that `run_id` exists, belongs to the Requirement in
  the URL, and is an unassigned not-yet-started run or an assigned active run
  (`phase=active`, including a pinned daemon that is operationally unavailable).
  Repeated cancellation of the same run remains idempotent. It returns
  `202 Accepted` with the public run projection after persisting
  `cancel_requested`. An unassigned run with no `session.start` becomes
  `phase=terminal` immediately and creates no `session.cancel` or other daemon
  command identity. An assigned run creates or reuses exactly one durable
  `session.cancel` for its pinned daemon but remains `phase=active` until a
  terminal runtime fact is projected. A `command_ack` only means the daemon
  durably recorded `session.cancel`; it is not cancellation completion. An unknown or Requirement-mismatched run returns HTTP `404` with generic
  error code `not_found` after normal authorization checks; it never falls back
  to a latest-run lookup.

`clarification/start` is the only identity-creating exception: it may resolve
the latest run to apply sequential create/reuse rules before returning `run_id`.
For every start, the conceptual order is authenticate/authorize, validate the
Requirement and persisted requester-message binding, enter per-Requirement
sequential-slot arbitration, and inspect the authoritative non-terminal
occupant. A matching occupant `start_message_id` selects same-start
idempotency: reuse its `run_id` and command identities, do not create a run or
reapply Draft → Discussing, and do not apply the original
`expected_state_version` as a new-mutation precondition. A different message
gets the existing-run sequential-slot conflict with no Requirement mutation,
new run, or `session.start`. Only an unoccupied slot is a genuinely new logical
start; only then does the server atomically validate `expected_state_version`
for run creation and any associated Requirement transition. Thus the state
version protects a new logical start, but does not invalidate an already-
committed matching retry or turn it into a stale new-start failure.
`GET /requirements/{requirement_id}/session` remains a latest-run read
convenience, but latest-run reads may guide UI presentation and MUST NOT
identify a dispatch or cancellation mutation. After `run_id` is known, every
such mutation includes it explicitly; a stale run ID is evaluated only against
that run and is never retargeted to a newer run. Unknown or Requirement-mismatched
run IDs on explicit run-scoped routes return HTTP `404` with generic error code
`not_found` after normal authorization checks; they never reveal cross-Requirement
run existence. Public application/read projections use `run_id`; existing
protocol `session_id` carries the same stable identity (`session_id = run_id`).
Protocol replay uses original message, run, and command identities; clients do
not call a generic command endpoint.

## Sequential clarification runs

Each clarification run is a server-owned record with one immutable Requirement
snapshot, one recorded `start_message_id`, and one immutable daemon pin after
assignment. Its application identity is `run_id`; the existing protocol's
`session_id` carries that same value (`session_id = run_id`). Its conceptual
fields are:

```text
run_id, requirement_id, start_message_id
 phase: awaiting_assignment | active | terminal
 status: starting | running | completed | unavailable
 cancel_requested: boolean
 created_at, updated_at, last_activity_at
```

`daemon_id` is internal and never part of the public projection. `phase` answers
whether this run still occupies the sequential clarification slot; `status`
describes coarse operational health/result; `cancel_requested` records user
intent. These fields are independent. For each Requirement, the
server/persistence authority derives one sequential clarification slot. At most
one non-terminal clarification run may occupy it: `phase=awaiting_assignment`
occupies it without a daemon or runtime execution, `phase=active` occupies it
with an assigned non-terminal run, and only `phase=terminal` releases it. This
is a lifecycle invariant derived from the existing phases, not another persisted
state machine. `phase=active` remains active even when
`status=unavailable` because a pinned daemon is disconnected or cancellation is
awaiting a terminal runtime fact. A new run with no eligible daemon remains
`phase=awaiting_assignment`, `status=unavailable`, and occupies the sequential
clarification slot. Daemon assignment is a conditional transition valid only
while that run remains the authoritative non-terminal `phase=awaiting_assignment`
occupant and has not been cancelled or closed. Under the same
server/persistence-authoritative decision, North verifies that eligibility and
atomically persists D's pin, the Requirement/run binding, immutable context, and
complete `session.start`; the run becomes `phase=active`, `status=starting`, and
continues to occupy the sequential clarification slot before dispatch. If the
run became terminal or otherwise ineligible before that commit, assignment fails
without persisting a daemon pin, binding, context, or command and does not
reactivate the run. `session.started` retains `phase=active` and changes only
coarse status to `running`.

A valid `clarification/start` may resolve the latest run before daemon selection
because it is the identity-creating operation. It may select an awaiting latest run for another daemon-selection attempt only
when all of these are true: `phase=awaiting_assignment`, `daemon_id = null`, no
`session.start` command was successfully created or dispatched, the run has not
been cancelled or otherwise closed, the request is the same logical start
attempt, and the incoming `message_id` equals its recorded `start_message_id`.
For that reusable unavailable attempt, the server retries daemon selection
without creating another run. In this slice, the same logical start attempt is
identified by the recorded `start_message_id` and an unclosed run; a new
persisted message is a new attempt. The response always returns the selected or
reused public run projection, including `run_id` and `start_message_id`. If a matching `start_message_id` already has a committed run identity for
an unclosed start attempt, concurrent or retried same-message starts resolve to
that run and its existing command identities: one serialized request may complete conditional assignment for an
awaiting run, but no request creates a second run or performs a second
assignment, lifecycle transition, or `session.start`. The state-version
precondition gates a new start mutation; it does not turn a matching
idempotent retry into a second mutation.

A latest `phase=active` run is assigned and non-terminal, including an assigned
run whose pinned daemon is disconnected or whose cancellation was requested. It
occupies the sequential clarification slot. A different start message returns
the canonical conflict and cannot release that slot. Later requester messages
use explicit run-scoped dispatch. A latest `phase=terminal` run has released the
slot; a new persisted eligible start message creates a new run. A repeated
request for an old terminal start message does not reactivate or retarget that
run.

### Sequential clarification slot arbitration

Concurrent identity-creating `clarification/start` requests for one Requirement
SHALL be serialized by server/persistence authority for their create,
reusable-run, conflict, and assignment decision. The contract is implementation
neutral: it does not require a particular lock, index, isolation level, or other
persistence primitive. It only requires that each outcome be equivalent to one
serialized ordering and that no browser timing can create two non-terminal runs.

For an empty slot, concurrent starts with the same eligible `message_id` create
or resolve one run identity, with at most one daemon assignment and one durable
`session.start` command identity. If no daemon is available, both logical
requests resolve to that one `phase=awaiting_assignment` run. Concurrent starts
with different eligible messages have one winner and one canonical existing-run
different-message conflict; the winner occupies the slot, the loser message
remains canonical conversation history, and it is not automatically dispatched,
rolled back, or converted into another run.

Concurrent retries of an existing `phase=awaiting_assignment` run using its
recorded `start_message_id` reuse that run. Daemon selection is serialized and
can commit at most one assignment and one `session.start`; a different message
cannot replace the awaiting run. An awaiting-run start retry racing explicit
cancellation is equivalent to one serialized order. Cancellation first makes the
run `phase=terminal` with `cancel_requested=true`, daemon pin null, and no
command, so retry cannot reactivate it. Assignment first makes it
`phase=active`, `status=starting`, with its daemon pin, run binding, immutable
context, and one durable `session.start`; cancellation then persists
`cancel_requested=true`, creates or reuses exactly one durable `session.cancel`,
and leaves the run active in the sequential clarification slot until a terminal
runtime fact. No hybrid terminal-and-assigned result is observable.

A newly created run receives a new `run_id`, current Requirement snapshot/revision,
`start_message_id`, repository set, daemon pin when selected, and independent
command/event sequence. The prior run remains immutable historical data. This
is a logical run contract, not a prescribed new table. Existing
`execution_sessions`/delivery storage may represent it while retaining current
durable delivery invariants.

Cancellation intent is not cancellation completion. For an unassigned run with
`daemon_id = null`, no `session.start`, and no runtime execution, cancellation
persists `cancel_requested=true`, immediately sets `phase=terminal`, creates no
command or command identity, and makes the run ineligible for reuse. For an
assigned `phase=active` run, cancellation persists the same intent, creates or
reuses exactly one durable `session.cancel`, and leaves the run `phase=active`
and in the sequential clarification slot until a terminal runtime fact is
durably projected.
A `command_ack` for `session.cancel` only confirms durable daemon recording; it
never closes the run or permits another start while the sequential clarification slot is occupied.

The existing terminal runtime facts that close an assigned run are
`session.completed` and `session.failed`, after normal session binding, identity,
and sequence validation and durable projection. `PiClarificationAdapter` maps a
confirmed successful runtime cancellation/termination to existing
`session.completed` with no readiness assessment; if runtime termination fails
or reports a terminal operational failure, it maps to existing `session.failed`.
No `session.cancelled` frame is introduced. The resulting terminal phase and
coarse status are projected through the same North event path, and
`cancel_requested` remains true.

The initial requester-message flow is two explicit HTTP calls:

1. `POST /requirements/{requirement_id}/conversation/messages` durably commits
   the requester message and returns its `message_id`. This call has no runtime
   side effect.
2. `POST /requirements/{requirement_id}/clarification/start` validates the
message binding, enters sequential-slot arbitration, and inspects the
authoritative occupant before applying a new-start state-version check. A
matching non-terminal occupant reuses its `run_id` and command identities,
without reapplying Draft → Discussing or the original
`expected_state_version`; a different message returns the existing-run
sequential-slot conflict with no Requirement mutation, new run, or
`session.start`. Only an unoccupied slot is a genuinely new logical start. For
that new start, the server atomically validates `expected_state_version`; a
stale token returns `409`, leaves the persisted message and Requirement
unchanged, and creates no run, daemon assignment, or command. If the
Requirement is Draft and the token is current, it applies the canonical
`begin_discussion` operation and creates a new run. If a daemon is selected,
the server assembles `session.start` from the immutable Requirement snapshot,
the North-selected deterministic conversation excerpt including the persisted
start message, and enabled repository metadata, then atomically persists the
daemon pin, immutable run context, and complete command before dispatch. If no
daemon is eligible, it retains the run with `daemon_id = null`,
`phase=awaiting_assignment`, `status=unavailable`, and no `session.start`,
returning HTTP `503` with `clarification_unavailable` and that public run
projection. The response includes `run_id` and `start_message_id`.

The start message is already present in `session.start` context and SHALL NOT
also create `message.send`. A repeated start with the same recorded
`start_message_id` reuses the same reusable unassigned run and, after assignment,
returns the same `run_id` and command identities. A different message conflicts
while a reusable unassigned attempt or latest `phase=active` run exists. Once
the latest run is `phase=terminal`, a new persisted eligible message starts a
new sequential run instead of retargeting the prior run.

For a later requester message, the UI first calls
`POST /requirements/{requirement_id}/conversation/messages` and retains the
returned `message_id`. It then places the explicit public `run_id` already
known from the start response or a read projection into the mutation URL; the
server never resolves latest-run identity while handling the request:
`POST /requirements/{requirement_id}/clarification/runs/{run_id}/messages/{message_id}/dispatch`.
The server verifies that this explicit run belongs to the Requirement and is
eligible to receive the message, and that the message belongs to the
Requirement's canonical conversation and is not the recorded start message. It
creates or reuses one durable `message.send` command containing that identity
and content, then dispatches/replays it through existing outbox, daemon journal,
ACK, sequence, and reconciliation semantics. Repeated calls reuse the
message-to-command mapping; duplicate delivery cannot submit one logical
requester message twice. No dispatch call creates another conversation message
or resolves the latest run.

If no assigned run can receive the command, the message remains canonical and
the operation/read model reports operational unavailability. A pinned daemon is
not replaced by another daemon. A message intended to start a run must use the
explicit start operation; clients do not classify it from transcript contents.

## Deterministic conversation context

North selects each run's conversation context from canonical persisted
conversation history. Persisted conversation order is authoritative. The
configured bound and size accounting are North-owned and deterministic. For
North 0.1, size accounting uses a fixed message count and/or UTF-8 byte size;
token-based accounting is deferred unless a later change defines a canonical
provider-independent tokenizer and tokenizer version as part of the selection
configuration. No Pi tokenizer or tokenizer abstraction is introduced here.

North selects the newest messages that fit the bound, always retains the
`start_message_id`, removes the oldest retained non-start messages first when
the bound is exceeded, and emits the retained messages in canonical persisted
order. Identical canonical persisted state and context configuration produce
the same excerpt. If retaining the required start message is necessary, the
bound applies after that required retention; this rule is deterministic and
provider-independent.

North stores the selected excerpt in the immutable run/start context before
`session.start` dispatch. Replay and reconstruction of the same immutable run
reuse that context rather than selecting messages again. Pi or any future
provider receives this North-selected excerpt and SHALL NOT choose canonical
messages using provider-specific relevance logic.

## Runtime boundary

The existing durable command journal remains outside the runtime adapter and
continues to pass `runtime_operation_id = command_id` into its narrow seam.
North 0.1 implements exactly one concrete clarification runtime integration:
`PiClarificationAdapter` in `north-daemon`, backed by Pi Agent. The adapter is
an implementation detail of `north-daemon`, not a North domain or wire-protocol
concept.

The daemon-private `ClarificationRuntime` seam is North-owned and
provider-neutral. Its shape is derived from North clarification execution
needs, not copied from Pi Agent's API or lifecycle. Conceptual layering:

```text
north-server
    |
    | existing north-protocol commands/events
    v
north-daemon
    |
    | North-owned ClarificationRuntime seam
    v
PiClarificationAdapter
    |
    v
Pi Agent
```

The seam's conceptual North-owned inputs are:

- stable operation identity;
- session/run identity;
- immutable Requirement snapshot;
- deterministic persisted conversation context;
- authorized, run-bound repository inspection handles/context; and
- cancellation/control intent.

Its conceptual North-owned outputs are:

- agent message;
- coarse product-visible activity;
- readiness assessment;
- completion; and
- operational failure.

These are conceptual responsibilities, not a prescribed Rust trait or set of
provider-shaped types. The seam SHALL NOT carry or expose:

- Pi SDK types;
- Pi event names;
- Pi session objects;
- Pi tool-call schemas;
- provider-specific lifecycle state;
- raw tool output;
- chain-of-thought or reasoning; or
- Pi-specific configuration structures.

`PiClarificationAdapter` owns all Pi-specific mapping. It translates Pi
callbacks and results into the North-neutral facts consumed by the daemon,
or drops details that have no North meaning. The daemon then emits only the
existing North protocol events; no Pi event is mirrored as a protocol frame.
The daemon and adapter report facts only: they cannot mutate Requirement state,
apply Requirement business transitions, or access server persistence directly.
`north-server` applies canonical conversation, readiness, and session
projections through existing North domain/persistence paths.

`PiClarificationAdapter` validates each server-supplied repository through the
run authorization, prepares and disposes its North-owned inspection workspace,
and passes only the resulting repository ID/full revision into Pi context. Pi
runs with tools, extensions, skills, and context files disabled; it therefore
cannot select arbitrary repositories, credentials, checkout paths, or server
persistence. While a run remains at `needs_clarification`, the immutable
server-selected start context is stored in the daemon's bounded local session
state so a later `message.send` can rehydrate it after daemon restart. This
state contains no checkout path or credential and is removed on completion,
failure, or cancellation.

For the 0.1 vertical slice, the execution path is:

1. Requirement clarification starts in North through the explicit authenticated
   server operation.
2. `north-server` assembles the immutable Requirement snapshot, deterministic
   conversation context, and run-bound repository metadata into the existing
   `session.start` command.
3. `north-daemon` receives that existing protocol command and invokes the
   North-owned `ClarificationRuntime` seam with North concepts.
4. `PiClarificationAdapter` invokes Pi using only repository inspection
   context/handles already authorized and bound to the run.
5. Pi produces agent-visible response, coarse activity, and readiness evidence;
   the adapter maps them into North-neutral facts.
6. `north-daemon` maps those facts to existing typed protocol events, while
   filtering Pi-only callbacks, raw tool output, and reasoning.
7. `north-server` persists canonical conversation, readiness, and session
   projections after durable event handling.

Exact Pi SDK/API call, process boundary, and low-level callback wiring are
implementation decisions inside `PiClarificationAdapter`; this repository does
not establish them. This change does not introduce a provider registry,
provider-selection API, or abstractions for hypothetical runtimes. It defines
the smallest stable seam needed to implement Pi cleanly and preserve future
runtime replacement without changing North-owned contracts. SDK dependencies
and Pi-specific types SHALL remain confined to `PiClarificationAdapter` within
`north-daemon`, and SHALL NOT appear in `north-server`, `north-domain`,
`north-protocol`, persistence, or browser contracts.

## Event projection and ordering

The server retains existing delivery identity/sequence validation and commits
an event projection before its terminal ACK:

| Event | Canonical effect | Requirement effect |
| --- | --- | --- |
| `session.started` | Retain `phase=active`, set coarse session status to `running`, and retain the runtime fact. | None. |
| `agent.message` | Insert one `agent` conversation message using the event message ID. | None. |
| `agent.activity` | Append one intentionally coarse activity record. | None. |
| `requirement.assessed` | Run existing readiness transaction; retain accepted/rejected immutable evidence and current pointer/read result. | Only the existing server/domain `Discussing → Ready` gate may promote. |
| `session.completed` | Set `phase=terminal`, coarse session status to `completed`, and retain safe summary. | None; completion does not imply Ready. |
| `session.failed` | Set `phase=terminal`, coarse status to `unavailable`, and retain a safe operational fact. | None; `recoverable` does not authorize retry or Requirement failure. |

The normal successful order is assessment then completion, but completion without
an accepted assessment is still a valid delivered fact: it leaves the
Requirement Draft/Discussing (or otherwise unchanged), exposes no synthetic
assessment, and reports that no current assessment was accepted. A later
assessment event is handled by its own sequence, session, revision, and domain
gates. A failure before assessment has the same no-Requirement-mutation rule.

A duplicate event with matching identity/payload returns its known ACK and does
not repeat a message insert, activity entry, session transition, or readiness
promotion. An identity/payload conflict remains a protocol error. A stale or
invalid assessment is a durable rejected result and receives
`event_ack(status=rejected)` only after rejection persistence; it is not a
runtime retry request.

## Minimal session read model

This slice exposes a small sequential projection, not the later retry state
machine:

```text
run_id, requirement_id, start_message_id
phase: awaiting_assignment | active | terminal
status: starting | running | completed | unavailable
cancel_requested: boolean
created_at, updated_at, last_activity_at
```

`phase` determines whether the run occupies the sequential clarification slot:
`awaiting_assignment` is an unassigned run with no runtime execution;
`active` is an assigned non-terminal run, including a pinned disconnected daemon
or cancellation intent awaiting terminal runtime projection; and `terminal` is
an unassigned cancellation or a durably projected `session.completed`/
`session.failed` fact. `status` describes coarse operational health/result and
may be `unavailable` in any phase. `cancel_requested` describes user intent and
does not itself change phase for an assigned run. `start_message_id` is safe
application identity needed to retry the identity-creating start after reload.

No `daemon_id`, credentials, checkout paths, provider details, attempt count,
retry budget, backoff, `Idle`/`Retrying`/final `Failed` policy, or automatic
`session.resume` is public or introduced here. Existing delivery/session rows
may be reused for internal ownership and watermarks without making later policy
values authoritative.

## Canonical HTTP read models

The server owns these reads; the browser never reconstructs them from a daemon
socket or an SSE stream:

| Read | Endpoint | Contract |
| --- | --- | --- |
| Requirement | existing `GET /requirements/{id}` | Complete structured Requirement, including `status`, `revision`, and `state_version`. |
| Conversation | existing `GET /requirements/{id}/conversation?offset=&limit=` | Persisted requester/agent/system messages in deterministic order; `next_offset` remains the pagination signal. |
| Latest readiness | new `GET /requirements/{id}/readiness` | Latest immutable assessment record, its outcome/rejection reason, repository IDs/full SHAs, and `current`; `current` is true only for an accepted assessment matching current revision, Ready state, and accepted state generation. Empty history returns no assessment, not a fabricated result. |
| Coarse activity | new `GET /requirements/{id}/activity?offset=&limit=` | Persisted product-visible summaries with stable ordering and bounded pages; never raw tool output or chain-of-thought. |
| Session/runtime | new `GET /requirements/{id}/session` | Latest clarification run for this Requirement, ordered by creation time; return `{ "session": null }` only when no run has ever existed. The public projection includes `run_id`, `requirement_id`, `start_message_id`, `phase`, `status`, `cancel_requested`, `created_at`, `updated_at`, and `last_activity_at`. `phase=awaiting_assignment` with `status=unavailable` identifies an unassigned run with no `session.start`; `phase=active` identifies an assigned non-terminal run even when its pinned daemon is disconnected or cancellation is requested; `phase=terminal` identifies an unassigned cancellation or durably projected `session.completed`/`session.failed` fact. `status` remains coarse operational health/result. No `daemon_id`, daemon credentials/details, checkout paths, or provider internals are exposed. This latest-run read is a UI convenience and never supplies implicit mutation identity. |

The existing Ready-only `review-packet` remains the human-review projection and
is not replaced by the latest-readiness endpoint.

## Clarification notification extension

`introduce-requirement-board` owns the authenticated `GET /events` SSE endpoint
and the base `requirement.changed` category. This change extends that same
producer after clarification canonical transactions with
`conversation.changed`, `readiness.changed`, `activity.changed`, and
`session.changed`. Each event contains only its category, Requirement identity,
and optional non-authoritative metadata. It is not a durable browser event log,
replay source, WebSocket, or second state store. `Last-Event-ID` is not required
for correctness. Missed, repeated, delayed, out-of-order, or reconnect-delivered
hints cause canonical HTTP refetch; they never patch state from event payloads.
No second SSE endpoint, event bus, or browser event store is introduced.

## Availability and cancellation

A valid explicit start may resolve the latest run before daemon selection only
to apply the sequential create/reuse rules. An unassigned run is reusable only
when `phase=awaiting_assignment`, `daemon_id = null`, no `session.start` was
created or dispatched, it has not been cancelled or closed, the request is the
same logical start attempt, and the incoming message is its recorded
`start_message_id`. A different message while that attempt remains reusable
returns the canonical conflict. A latest `phase=active` run is assigned and
non-terminal, including an assigned run temporarily unavailable because its
daemon disconnected or awaiting completion after cancellation; it rejects a
different start message and retains the sequential clarification slot. Start returns the public
run projection, including `run_id` and `start_message_id`.

If the latest run is `phase=terminal`, a new persisted eligible requester
message and explicit start create a new run. The new run receives a new `run_id`,
current Requirement snapshot/revision, `start_message_id`, repository set, and
eventual daemon pin; the prior run is immutable history. Requirement edits
between runs therefore affect only the new run's snapshot. No eligible daemon
means the newly selected run remains `daemon_id = null`, `phase=awaiting_assignment`,
`status=unavailable`, has no `session.start` command, and returns HTTP `503`
with `clarification_unavailable`; no runtime event, Requirement failure, attempt
consumption, or implicit later daemon selection is fabricated.

Once `daemon_id != null`, the run remains pinned to that daemon. If it
disconnects, the server does not clear or migrate the pin; the run remains
`phase=active`, durable commands remain replayable, and the public read reports
`status=unavailable` until existing reconnect/delivery recovery resumes. No retry
budget, attempt accounting, server backoff, automatic `session.resume`, or final
execution failure policy is added here.

Dispatch and cancellation require explicit `run_id`. The server first performs
normal Requirement authorization, then looks up the run constrained by that
Requirement. An unknown or Requirement-mismatched run returns HTTP `404` with
generic error code `not_found`; it never leaks cross-Requirement run existence.
Dispatch is eligible only for an assigned `phase=active` run with
`cancel_requested=false`; an `awaiting_assignment`, cancellation-pending, or
`terminal` run receives its run-scoped canonical conflict/unavailability
result without a command. An active pinned daemon may be temporarily
unavailable, but its durable `message.send` remains bound to that run for
recovery and replay. A cancellation-pending run remains active in the sequential clarification slot,
but later message dispatch MUST fail/conflict. If requester message persistence
commits before cancellation wins a race, that message remains canonical; a
subsequent dispatch MUST fail/conflict and MUST NOT create `message.send` or
roll back the persisted message.

Cancellation intent is separate from cancellation completion. For an unassigned
run with `daemon_id = null`, no `session.start`, and no runtime execution,
cancellation persists `cancel_requested=true`, immediately sets
`phase=terminal`, creates no `session.cancel` or other daemon command identity,
and makes the run ineligible for reuse. A later eligible persisted message may
start a new sequential run.

For an assigned `phase=active` run, cancellation persists
`cancel_requested=true`, creates or reuses exactly one durable `session.cancel`
for its pinned daemon, and leaves the run `phase=active` in the sequential clarification slot.
A repeated request is idempotent; later message dispatch is not legal while
`cancel_requested=true`. `command_ack` only means the daemon durably
recorded `session.cancel`; it is not runtime cancellation completion and does
not permit a new run. A later valid `session.completed` or `session.failed`
event, after normal binding/identity/sequence validation and durable projection,
sets `phase=terminal`; `session.completed` yields coarse `status=completed`, and
`session.failed` yields `status=unavailable`. If cancellation succeeds, the
Pi adapter maps confirmed runtime termination to `session.completed`; if
termination fails, it maps the existing terminal operational failure to
`session.failed`. No `session.cancelled` frame is introduced. The prior run
then remains immutable history and a later eligible start may create a new run.
No retry or final-failure policy is added.

## Dependency graph

```text
introduce-requirement-board
  -> board/list/create/minimal read-only detail
  -> base authenticated GET /events + requirement.changed

introduce-local-repository-inspection
  -> introduce-agent-requirement-clarification
introduce-requirement-board
  -> clarification SSE-category extension

introduce-requirement-board + introduce-agent-requirement-clarification
     -> introduce-requirement-conversation-workspace
     canonical successor; extends the existing detail shell

introduce-runtime-retry-and-failure-state
  -> later optional status/read-model extension
```

Board does not depend on local inspection or clarification. Clarification
extends, but does not recreate, Board's shared `/events` infrastructure.
