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
  including `run_id`. A valid start with no eligible daemon returns `503 Service
  Unavailable` with error code `clarification_unavailable`, the canonical
  Requirement, and the unassigned `status=unavailable` run projection. A stale
  state version returns the canonical `409` conflict before any run/command
  mutation; the message remains persisted.
- `POST /requirements/{requirement_id}/clarification/runs/{run_id}/messages/{message_id}/dispatch`
  has no message body. It validates that `run_id` exists, belongs to the
  Requirement in the URL, is assigned and active (`starting` or `running`,
  including pinned operational unavailability), and that the persisted requester
  message belongs to this Requirement's canonical conversation, is eligible for
  that run, and is not the recorded start message. It returns `202 Accepted`
  after creating or reusing exactly one durable `message.send` mapping. A pinned
  offline daemon keeps that command durable and reports operational
  unavailability; an unassigned run returns `503 clarification_unavailable`
  without creating a command. It never creates another conversation message or
  resolves a newer latest run.
- `POST /requirements/{requirement_id}/clarification/runs/{run_id}/cancel` has no
  message body. It validates that `run_id` exists, belongs to the Requirement in
  the URL, and is an unassigned not-yet-started run or an assigned active run
  (`starting` or `running`, including pinned operational unavailability);
  repeated cancellation of the same run remains idempotent. It returns
  `202 Accepted` with the public run projection after persisting
  `cancel_requested`. If the run is assigned, the server also creates or reuses
  exactly one durable `session.cancel` command for its pinned daemon. If the run
  is unassigned, cancellation is server-owned run state only: no
  `session.cancel` command and no command identity are created. Repeated calls
  return that run's persisted cancellation state. No run returns HTTP `404` with
  the `clarification_not_started` contract.

`clarification/start` is the only identity-creating exception: it may resolve
the latest run to apply sequential create/reuse rules before returning `run_id`.
`GET /requirements/{requirement_id}/session` remains a latest-run read
convenience, but latest-run reads may guide UI presentation and MUST NOT
identify a dispatch or cancellation mutation. After `run_id` is known, every
such mutation includes it explicitly; a stale run ID is evaluated only against
that run and is never retargeted to a newer run. Public application/read
projections use `run_id`; existing protocol `session_id` carries the same stable
identity (`session_id = run_id`). Protocol replay uses original message, run,
and command identities; clients do not call a generic command endpoint.

## Sequential clarification runs

Each clarification run is a server-owned record with one immutable Requirement
snapshot, one recorded `start_message_id`, and one immutable daemon pin after
assignment. Its application identity is `run_id`; the existing protocol's
`session_id` carries that same value (`session_id = run_id`). Its conceptual
fields are:

```text
run_id, requirement_id, start_message_id
 daemon_id: nullable until assignment
 status: starting | running | completed | unavailable
 cancel_requested: boolean
 created_at, updated_at, last_activity_at
```

A valid `clarification/start` may resolve the latest run before daemon selection
because it is the identity-creating operation. It reuses the latest run only
when all of these are true: `daemon_id = null`, no `session.start` command was
successfully created or dispatched, the run has not been cancelled or otherwise
closed, the request is the same logical start attempt, and the incoming
`message_id` equals its recorded `start_message_id`. For that reusable
unavailable attempt, the server retries daemon selection without creating
another run. In this slice, the same logical start attempt is identified by the
recorded `start_message_id` and an unclosed run; a new persisted message is a
new attempt. The response always returns the selected/reused public `run_id`.

If the latest run is assigned and active (`starting` or `running`, including an
assigned run that is operationally unavailable because its daemon disconnected),
a different start message returns the canonical conflict; later requester
messages use the explicit run-scoped dispatch operation. If the latest run is
completed, cancelled, or otherwise explicitly terminal/inapplicable for start
reuse, a new persisted eligible start message creates a new run. A repeated
request for the old terminal start message does not reactivate or retarget that
run.

A newly created run receives a new `run_id`, current Requirement snapshot/revision,
start message, repository set, daemon pin when selected, and independent
command/event sequence. The prior run remains immutable historical data. This
is a logical run contract, not a prescribed new table. Existing
`execution_sessions`/delivery storage may represent it while retaining current
durable delivery invariants.

The initial requester-message flow is two explicit HTTP calls:

1. `POST /requirements/{requirement_id}/conversation/messages` durably commits
   the requester message and returns its `message_id`. This call has no runtime
   side effect.
2. `POST /requirements/{requirement_id}/clarification/start` validates that
   message ID and `expected_state_version` before any run or command mutation.
   If the Requirement is Draft, it applies the canonical `begin_discussion`
   operation; a stale token returns `409` and leaves the persisted message and
   Requirement unchanged. It then applies the sequential reuse/new-run rules
   above before daemon selection. If the latest run is terminal/inapplicable
   and this is a new eligible start message, the server creates a new run. If a
   reusable unassigned attempt is present, it reuses that run. If a daemon is
   selected, the server assembles `session.start` from the immutable Requirement
   snapshot, the North-selected deterministic conversation excerpt including the
   persisted start message, and enabled repository metadata, then atomically
   persists the daemon pin, immutable run context, and complete command before
   dispatch. If no daemon is eligible, it retains the run with
   `daemon_id = null`, `status=unavailable`, and no `session.start`, returning
   `503 clarification_unavailable` with that run projection. The response
   includes the public `run_id`.

The start message is already present in `session.start` context and SHALL NOT
also create `message.send`. A repeated start with the same recorded
`start_message_id` reuses the same reusable unassigned run and, after assignment,
returns the same `run_id` and command identities. A different message conflicts
while a reusable unassigned attempt or assigned active run exists. Once the
latest run is completed, cancelled, or otherwise explicitly
terminal/inapplicable, a new persisted eligible message starts a new sequential
run instead of retargeting the prior run.

For a later requester message, the UI first calls
`POST /requirements/{requirement_id}/conversation/messages` and retains the
returned `message_id`. It then uses the known `run_id` from the start response
or the canonical latest-session read to call
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
configured bound and size accounting are North-owned and deterministic; the
bound may be a fixed message count, byte/token budget, or another deterministic
configuration detail and is not fixed by this change.

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
| `session.started` | Set coarse session status to `running`; retain runtime fact. | None. |
| `agent.message` | Insert one `agent` conversation message using the event message ID. | None. |
| `agent.activity` | Append one intentionally coarse activity record. | None. |
| `requirement.assessed` | Run existing readiness transaction; retain accepted/rejected immutable evidence and current pointer/read result. | Only the existing server/domain `Discussing → Ready` gate may promote. |
| `session.completed` | Set coarse session status to `completed` and retain safe summary. | None; completion does not imply Ready. |
| `session.failed` | Set coarse status to `unavailable` and retain a safe operational fact. | None; `recoverable` does not authorize retry or Requirement failure. |

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

This slice exposes a coarse projection, not the later retry state machine:

```text
status: starting | running | completed | unavailable
cancel_requested: boolean
run_id, requirement_id, updated_at, last_activity_at
```

`unavailable` covers no eligible daemon and runtime failure facts. It is
operational status, not Requirement `Failed`. No attempt count, retry budget,
backoff, `Idle`/`Retrying`/final `Failed` policy, or automatic `session.resume`
is introduced here. Existing delivery/session rows may be reused for ownership
and watermarks without making those later policy values authoritative.

## Canonical HTTP read models

The server owns these reads; the browser never reconstructs them from a daemon
socket or an SSE stream:

| Read | Endpoint | Contract |
| --- | --- | --- |
| Requirement | existing `GET /requirements/{id}` | Complete structured Requirement, including `status`, `revision`, and `state_version`. |
| Conversation | existing `GET /requirements/{id}/conversation?offset=&limit=` | Persisted requester/agent/system messages in deterministic order; `next_offset` remains the pagination signal. |
| Latest readiness | new `GET /requirements/{id}/readiness` | Latest immutable assessment record, its outcome/rejection reason, repository IDs/full SHAs, and `current`; `current` is true only for an accepted assessment matching current revision, Ready state, and accepted state generation. Empty history returns no assessment, not a fabricated result. |
| Coarse activity | new `GET /requirements/{id}/activity?offset=&limit=` | Persisted product-visible summaries with stable ordering and bounded pages; never raw tool output or chain-of-thought. |
| Session/runtime | new `GET /requirements/{id}/session` | Latest clarification run for this Requirement, ordered by creation time; return `{ "session": null }` only when no run has ever existed. An attempted start with no eligible daemon returns that unassigned run with `status=unavailable`. An assigned/offline run keeps its pinned daemon internally and returns the same run with `status=unavailable`; completed or cancelled runs remain readable until a newer run exists. After a new sequential run is created, it is the latest result while prior runs remain server-side history. The public projection exposes `run_id`, status, cancellation intent, and timestamps, not `daemon_id` or daemon details. This latest-run read is a UI convenience and never supplies implicit mutation identity. |

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
when `daemon_id = null`, no `session.start` was successfully created or
dispatched, it has not been cancelled or closed, the request is the same logical
start attempt, and the incoming message is its recorded `start_message_id`. A
different message while that attempt remains reusable returns the canonical
conflict. A latest assigned active run (including an assigned run temporarily
unavailable because its daemon disconnected) also rejects a different start
message, preventing competing runs. Start returns the public `run_id` of the
created or reused run.

If the latest run is completed, cancelled, or otherwise explicitly
terminal/inapplicable, a new persisted eligible requester message and explicit
start create a new run. The new run receives a new `run_id`, current Requirement
snapshot/revision, start message, repository set, and eventual daemon pin; the
prior run is immutable history. Requirement edits between runs therefore affect
only the new run's snapshot. No daemon means the newly selected run remains
`daemon_id = null`, `status=unavailable`, has no `session.start` command, and
returns `503 clarification_unavailable`; no runtime event, Requirement failure,
attempt consumption, or implicit later daemon selection is fabricated.

Once `daemon_id != null`, the run remains pinned to that daemon. If it
disconnects, the server does not clear or migrate the pin; durable commands
remain replayable and the public read reports `status=unavailable` until
existing reconnect/delivery recovery resumes. No retry budget, attempt
accounting, server backoff, automatic `session.resume`, or final execution
failure policy is added here.

Dispatch and cancellation require explicit `run_id`. The server validates that
the run exists, belongs to the Requirement in the URL, and is eligible for the
requested operation before creating or reusing a command. Cancellation persists
`cancel_requested` without changing Requirement lifecycle, content, revision, or
state_version. For an assigned eligible run, the server creates or reuses
exactly one durable `session.cancel` command for its pinned daemon; repeated
requests reuse that command/result and runtime cancellation occurs at most once.
For an unassigned eligible run, cancellation is server-owned run state only: it
creates no `session.cancel` command, no command identity, and no fabricated
daemon pin. An ineligible terminal run returns its run-scoped canonical result
(or existing cancellation state) without creating a command. A stale mutation
for run A therefore cannot resolve or affect newer run B. The cancelled run
remains historical and ineligible for start reuse; a later new eligible message
may start a new sequential run. No retry or final-failure policy is added.

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
  -> introduce-requirement-conversation-ui
     extends the existing detail shell

introduce-runtime-retry-and-failure-state
  -> later optional status/read-model extension
```

Board does not depend on local inspection or clarification. Clarification
extends, but does not recreate, Board's shared `/events` infrastructure.
