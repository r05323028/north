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
| Browser SSE producer and notification endpoint | `north-server` in this change |
| Browser rendering/refetch | board and conversation UI changes |
| Retry budget, attempts, server backoff, final execution failure | later `introduce-runtime-retry-and-failure-state` |

## One clarification run

A run has one immutable requirement snapshot and one pinned daemon owner.
`session.start` is the first durable command for that run. Its envelope's
`session_id` binds the payload; the `SessionStart` payload does not gain a
second session identity field. The server persists owner and run repository
IDs before dispatch, and reconnect/replay reuses the original command
identity, sequence, and payload.

The initial requester-message flow is:

1. Validate and durably append the requester message using the existing
   conversation persistence path.
2. If the Requirement is Draft, apply the explicit domain
   `begin_discussion` operation with the caller's current
   `expected_state_version`. This is a business transition, not an inference
   from transcript text. A stale expected state returns the canonical conflict
   and creates no session.start command; the already-persisted message remains
   history. If no daemon is available, the persisted message and canonical
   Discussing state remain; the run is operationally unavailable, never
   Requirement-failed.
3. Assemble `session.start` from the current immutable Requirement snapshot,
   bounded/relevant conversation excerpt including that persisted message, and
   the enabled repository catalog. Persist the session owner/context and full
   command envelope before dispatch.
4. Do **not** also create `message.send` for that initial message. It is already
   present in the start context.

For a later requester message in an existing run:

1. Commit the message first and retain its returned `message_id`.
2. Create one durable `message.send` command containing that `message_id` and
   content. Store/reuse the server message-to-command mapping.
3. Dispatch or replay the stored command through existing outbox, daemon
   journal, ACK, sequence, and reconciliation semantics.

If command dispatch is unavailable after message commit, history remains the
canonical record and the session read model reports operational unavailability.
A pinned owner is not replaced by another daemon. A retry of the same logical
message reuses its command/message identities; duplicate delivery cannot submit
one requester message to the runtime twice.

If no run exists, the next explicit clarification start treats its first
persisted message as start context. The server never sends a message to the
runtime before that message exists in canonical conversation history.

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
| Session/runtime | new `GET /requirements/{id}/session` | The minimal run projection above; return `{ "session": null }` when no run exists. A no-eligible start creates an unassigned projection with `status=unavailable`. |

The existing Ready-only `review-packet` remains the human-review projection and
is not replaced by the latest-readiness endpoint.

## Browser notification producer

`north-server` emits notifications only after the corresponding canonical
transaction commits through one authenticated SSE endpoint,
`GET /events`, with an optional `requirement_id` filter. Minimal categories are:

- `requirement.changed`;
- `conversation.changed`;
- `readiness.changed`;
- `activity.changed`; and
- `session.changed`.

Each event contains only a category and requirement identity (plus optional
non-authoritative notification metadata). It is not a durable browser event
log, a replay source, a WebSocket, or a second state store. Missed, repeated,
out-of-order, or reconnect-delivered hints cause a canonical HTTP refetch;
they never patch Requirement state from the event payload. Board/list and
detail consumers share this producer and endpoint rather than creating
separate invalidation systems.

## Availability and cancellation

A new run with no eligible daemon creates or retains one unassigned coarse
run projection with `status=unavailable`, returns an explicit operational-
unavailable result (HTTP 503 with the `clarification_unavailable` error code), and creates
no `session.start` command until a later explicit start. It never fabricates a
daemon event or Requirement failure. An existing pinned run keeps its owner; if
that owner is offline, commands remain durable/replayable and the read model
stays operationally unavailable until delivery resumes. No automatic retry or
live migration is added.

Cancellation creates one durable `session.cancel` command for the pinned run.
Repeated cancellation requests reuse the logical cancellation identity or the
known terminal command result and never invoke the runtime twice. Cancellation
only affects the run; it does not mutate Requirement content, revision, or
lifecycle truth.

## Dependency graph

```text
local inspection  ->  clarification orchestration
clarification     ->  shared server SSE + canonical reads
clarification     ->  board and conversation/detail consumers
runtime retry     ->  later optional status/read-model extension
```
