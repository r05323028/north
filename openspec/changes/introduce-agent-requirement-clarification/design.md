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
| Agent SDK/runtime invocation | one internal `north-daemon` adapter |
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
  `202 Accepted` with the canonical Requirement and public run projection. A
  valid start with no eligible daemon returns `503 Service Unavailable` with
  error code `clarification_unavailable`, the canonical Requirement, and the
  unassigned `status=unavailable` run projection. A stale state version returns
  the canonical `409` conflict before any run/command mutation; the message
  remains persisted.
- `POST /requirements/{requirement_id}/clarification/messages/{message_id}/dispatch`
  has no message body. It validates the persisted requester message, its
  Requirement/conversation binding, ownership of the current assigned active
  run, and that it is not the recorded start message. It returns `202 Accepted`
  after creating or reusing exactly one durable `message.send` mapping. A pinned
  offline daemon keeps that command durable and reports operational
  unavailability; an unassigned/no-owner run returns
  `503 clarification_unavailable` without creating a command. It never creates
  another conversation message.
- `POST /requirements/{requirement_id}/clarification/cancel` has no message
  body and targets the latest applicable run. It returns `202 Accepted` with
  the public run projection after creating or reusing the durable
  `session.cancel` command for an assigned run. Repeated calls reuse the same
  logical command/result. For an unassigned run, the server persists the same
  run-bound cancellation intent with a stable command identity but does not
  dispatch without an owner; cancellation makes that run inapplicable for a
  later start reuse. No run returns `404 clarification_not_started`.

Start and dispatch responses never expose daemon credentials, checkout paths,
or unnecessary daemon details. Operation retries use the original message,
run, and command identities; clients do not call a generic command endpoint.

## One clarification run

A clarification run is a server-owned record with one immutable Requirement
snapshot and one immutable daemon pin after assignment. Its conceptual fields
are:

```text
id, requirement_id, start_message_id
 daemon_id: nullable until assignment
 status: starting | running | completed | unavailable
 cancel_requested: boolean
 created_at, updated_at, last_activity_at
```

A valid `clarification/start` creates or reuses this run before daemon
selection. `session.start` is the first daemon command only after an owner is
selected. Its envelope's `session_id` binds the payload; the `SessionStart`
payload does not gain a second session identity field. Reconnect/replay reuses
the original command identity, sequence, and payload.

This is a logical run contract, not a prescribed new table. Existing
`execution_sessions`/delivery storage may represent the run and its nullable
owner while retaining current durable delivery invariants.

The initial requester-message flow is two explicit HTTP calls:

1. `POST /requirements/{requirement_id}/conversation/messages` durably commits
   the requester message and returns its `message_id`. This call has no runtime
   side effect.
2. `POST /requirements/{requirement_id}/clarification/start` validates that
   message ID and `expected_state_version` before any run or command mutation.
   If the Requirement is Draft, it applies the canonical `begin_discussion`
   operation; a stale token returns `409` and leaves the persisted message and
   Requirement unchanged. It then creates/reuses the latest eligible
   unassigned run before daemon selection. If a daemon is selected, the server
   assembles `session.start` from the immutable Requirement snapshot, bounded
   conversation context including the persisted start message, and enabled
   repository metadata, then atomically persists the daemon pin, run context,
   and complete command before dispatch. If no daemon is eligible, it retains
   the run with `daemon_id = null`, `status=unavailable`, and no
   `session.start`, returning `503 clarification_unavailable` with that run
   projection.

The start message is already present in `session.start` context and SHALL NOT
also create `message.send`. A repeated start for the same recorded start
message reuses the same unassigned/assigned run and command identities. Once a
run is assigned, completed, or cancelled, it is not a new-start target; a
different start message is rejected rather than creating a competing run.

For a later requester message, the UI first calls
`POST /requirements/{requirement_id}/conversation/messages` and retains the
returned `message_id`, then calls
`POST /requirements/{requirement_id}/clarification/messages/{message_id}/dispatch`.
The server verifies that the message and current run belong to the Requirement
and caller's authorized conversation/run context. It creates or reuses one
durable `message.send` command containing that identity and content, then
dispatches/replays it through existing outbox, daemon journal, ACK, sequence,
and reconciliation semantics. Repeated calls reuse the message-to-command
mapping; duplicate delivery cannot submit one logical requester message twice.
No dispatch call creates another conversation message.

If no assigned run can receive the command, the message remains canonical and
the operation/read model reports operational unavailability. A pinned owner is
not replaced by another daemon. A message intended to start a run must use the
explicit start operation; clients do not classify it from transcript contents.

## Runtime boundary

The existing durable command journal remains outside the runtime adapter and
continues to pass `runtime_operation_id = command_id` into its narrow seam.
Behind that seam, define one daemon-private North-facing runtime interface:

- input: stable operation/session identity, server-assembled North-neutral
  requirement/conversation/repository context, cancellation handle, and
  session/task checkout handles supplied by local inspection when needed;
- output: North-neutral facts for agent message, coarse activity, readiness
  assessment, completion, or failure; and
- control: one explicit cancellation operation and local recovery/reattachment
  mechanics only.

The interface must not mirror a provider SDK's callback graph, expose provider
objects, or return raw tool/chain-of-thought records. One concrete adapter is
implemented first. SDK dependencies remain in `north-daemon`; no SDK type
crosses into `north-server`, `north-domain`, or `north-protocol`.

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
session_id, requirement_id, updated_at, last_activity_at
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
| Session/runtime | new `GET /requirements/{id}/session` | Latest clarification run for this Requirement, ordered by creation time; return `{ "session": null }` only when clarification has never been started. An attempted start with no eligible daemon returns the unassigned run with `status=unavailable`. An assigned/offline run keeps its pinned daemon internally and returns the same run with `status=unavailable`; completed runs remain readable as the latest run. The public projection exposes status, cancellation intent, and timestamps, not `daemon_id` or daemon details. |

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

A valid explicit start creates or reuses the latest applicable run before daemon
selection. If no eligible daemon is available, the run remains present with
`daemon_id = null`, `status=unavailable`, and no `session.start` command. The
server returns `503 clarification_unavailable` together with the public run
projection and any valid canonical Requirement transition already committed.
It never fabricates a runtime event or Requirement failure, consumes an attempt,
or selects another daemon implicitly. A later daemon selection requires another
explicit start request; an unassigned run that has never been assigned,
dispatched, or cancelled is reused, so repeated requests cannot create competing
runs.

Once `daemon_id != null`, the run remains pinned to that daemon. If it
disconnects, the server does not clear or migrate the owner; durable commands
remain replayable and the public read reports `status=unavailable` until
existing reconnect/delivery recovery resumes. No retry budget, attempt
accounting, server backoff, automatic `session.resume`, or final execution
failure policy is added here.

Cancellation targets the latest applicable clarification run. For an assigned
run, the server sets `cancel_requested` and creates or reuses one durable
`session.cancel` command for its pinned daemon. For an unassigned run, the
server persists the same stable run-bound cancellation intent without dispatch;
its nullable owner is never fabricated. Repeated requests reuse the command or
known terminal result and never invoke runtime cancellation twice. Cancellation
makes an unassigned run inapplicable for later start reuse rather than silently
clearing the intent. Cancellation changes only run facts; it does not mutate
Requirement lifecycle, content, revision, or state_version.

## Dependency graph

```text
introduce-requirement-board
  -> base authenticated GET /events + requirement.changed

introduce-local-repository-inspection
  -> clarification orchestration
introduce-requirement-board
  -> clarification SSE-category extension

introduce-requirement-board + introduce-agent-requirement-clarification
  -> conversation/detail HTTP/SSE consumer

introduce-runtime-retry-and-failure-state
  -> later optional status/read-model extension
```

Board does not depend on local inspection or clarification. Clarification
extends, but does not recreate, Board's shared `/events` infrastructure.
