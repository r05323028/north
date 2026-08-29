## Purpose

Runs one server-authorized clarification execution through the existing North
protocol and durable delivery seams, persists safe runtime facts, and applies
readiness without moving business authority to the daemon.

## ADDED Requirements

### Requirement: Consume the existing North protocol contract

The clarification runtime SHALL use the existing `north-protocol` command/event
families: `session.start`, `session.cancel`, `session.resume`, `message.send`,
`session.started`, `agent.message`, `agent.activity`,
`requirement.assessed`, `session.completed`, and `session.failed`. It SHALL
reuse existing envelope IDs, directional sequences, ACK-after-commit,
reconciliation, and identity-conflict behavior. It SHALL NOT introduce a
second wire schema, alternate ACK names, daemon-event cursors in
`session.resume`, or protocol types in the domain crate.

#### Scenario: Runtime work uses established delivery

- **WHEN** a clarification command or runtime event is sent
- **THEN** it crosses the existing durable outbox/journal and reconciliation paths with the canonical frame name and identity semantics

### Requirement: Server assembles and pins one complete run context

Before the first `session.start` dispatch, the server SHALL select an eligible
connected daemon, persist the session's `daemon_id`, bind the session to one
Requirement, persist the run repository IDs, and persist the complete command
envelope. `session.start` SHALL contain the immutable Requirement snapshot, a
bounded/relevant persisted conversation excerpt, and enabled repository
metadata only. The envelope's `session_id` is the run identity; the payload
SHALL NOT gain a duplicate session field. Credentials, checkout paths,
database handles, and `north-domain` types SHALL NOT cross the daemon boundary.
A run's repository set and requirement snapshot SHALL NOT be silently
retargeted on later command replay. Any user-driven Draft → Discussing
transition used by start orchestration SHALL honor the current
`expected_state_version` contract; `revision` is the content snapshot token,
not the write precondition.

#### Scenario: Start sees server context, not storage

- **WHEN** the server dispatches `session.start`
- **THEN** the daemon receives the server-selected requirement/conversation/repository context and no credential or persistence access

#### Scenario: Reconnect preserves owner and snapshot

- **WHEN** the pinned daemon reconnects after the start command was outboxed
- **THEN** the server replays the stored command to that same daemon with its original identity, sequence, and payload

#### Scenario: Stale discussion start cannot create a run

- **WHEN** an initial-message start uses an older `expected_state_version`
- **THEN** the server returns the canonical conflict, leaves Requirement state unchanged, keeps the already-persisted message, and creates no daemon command

### Requirement: Requester messages are durable before runtime dispatch

For every requester message that is sent to a runtime, the server SHALL first
commit it through the canonical conversation persistence path and obtain its
stable message identity. Only then SHALL it create or reuse one durable
`message.send` command containing that identity and content. Dispatch/replay
SHALL use existing server outbox and daemon journal identity mapping. A
duplicate/replayed command SHALL submit the logical message to the runtime at
most once. If dispatch is unavailable after message commit, the message SHALL
remain in canonical history and the run SHALL report operational availability
separately.

The first requester message that starts a run SHALL be included in
`session.start` conversation context and SHALL NOT also be sent as
`message.send`. Later messages in that run SHALL use `message.send`; if no run
exists, the next explicit start treats its first message as start context.

#### Scenario: Message cannot exist only at daemon

- **WHEN** a requester posts a message for clarification
- **THEN** the conversation row commits the message before any command can reach the daemon

#### Scenario: Replayed message is submitted once

- **WHEN** reconnect or daemon restart replays the same `message.send` command
- **THEN** the original command/message identities are reused and the runtime receives no second logical requester submission

#### Scenario: Initial message is not duplicated

- **WHEN** message M starts a new run
- **THEN** M appears in persisted conversation history and the `session.start` excerpt, with no separate `message.send` for M

### Requirement: Runtime boundary is North-facing and single-implementation

The daemon SHALL invoke one concrete runtime adapter behind its existing
stable-operation durable dispatch seam. The internal interface SHALL accept
North-neutral session input and cancellation/control plus stable operation
identity, and SHALL return North-neutral agent message, coarse activity,
assessment, completion, and failure facts. It SHALL NOT mirror a provider SDK's
lifecycle, expose SDK/provider values, or give the runtime business write
access. SDK dependencies SHALL remain confined to `north-daemon`;
`north-domain` and `north-protocol` SHALL remain SDK-independent. The daemon
SHALL retain only local transport reconnect, journal recovery, and runtime
reattachment authority.

#### Scenario: Provider details stay behind the seam

- **WHEN** the concrete agent SDK emits provider-specific callbacks or tool records
- **THEN** the adapter maps them to North-neutral facts or drops them before any server/protocol projection

### Requirement: Runtime events project canonically after durable handling

For well-formed, session-bound runtime events, the server SHALL retain the
existing event identity/sequence checks, apply one idempotent projection, and
send the terminal event ACK only after that projection commits:

- `session.started` sets coarse session status to `running`;
- `agent.message` appends one persisted `agent` conversation message;
- `agent.activity` appends one coarse product-visible activity record;
- `session.completed` sets coarse status to `completed` without changing the
  Requirement; and
- `session.failed` sets coarse status to `unavailable` as an operational fact
  without choosing retry or mutating the Requirement.

A matching duplicate/replay SHALL return the known ACK without repeating the
projection. A different payload or identity reuse remains a protocol conflict.
Raw tool output and chain-of-thought SHALL never enter message/activity read
models.

#### Scenario: Agent message becomes canonical history

- **WHEN** a valid `agent.message` event is committed
- **THEN** the existing conversation HTTP read returns it, and a duplicate event does not add a second message

#### Scenario: Completion does not mean Ready

- **WHEN** `session.completed` arrives with no accepted assessment
- **THEN** the session reads completed, the Requirement remains unchanged, and no synthetic readiness result is created

### Requirement: Assessment handling is revision-bound and atomic

For `requirement.assessed`, the server SHALL use the existing typed conversion
and canonical persistence/domain path. It SHALL validate session/Requirement
binding, event identity and sequence, repository identity/run membership, and
`requirement_revision` against the current Requirement revision before
applying domain readiness gates. Accepted evidence, any valid `Discussing` →
`Ready` transition, dedupe state, and the accepted Ready-generation
`state_version` SHALL commit atomically before `event_ack(status=accepted)`.
A well-formed, sequence-valid stale or domain-gated assessment SHALL commit
durable rejection evidence and `event_ack(status=rejected)` without changing
Requirement status, revision, or `state_version`. Malformed payloads, identity
conflicts, and sequence gaps retain the existing protocol-error/no-ACK rules.
Duplicate accepted/rejected assessment events SHALL be inert.

#### Scenario: Edit makes an in-flight assessment stale

- **WHEN** a run starts with revision N, the Requirement is edited to revision N+1, and the run reports an assessment for N
- **THEN** the server durably rejects the assessment after canonical validation, ACKs the rejection, and leaves the current Requirement unchanged

#### Scenario: Accepted assessment has one business effect

- **WHEN** a current-revision Ready assessment passes server/domain gates
- **THEN** evidence and the single Ready promotion commit together, and a replay cannot promote twice

### Requirement: Completion and failure facts have explicit semantics

A normal runtime SHOULD emit `requirement.assessed` before
`session.completed`, but a well-formed session completion without an accepted
assessment SHALL still be a valid session fact. It SHALL leave the Requirement
at its current lifecycle state and expose no accepted current assessment. A
`session.failed` event before assessment SHALL set only coarse operational
unavailability; its `recoverable` value is a daemon-local recovery fact. No
completion or failure event SHALL synthesize readiness, consume a business
retry budget, or change Requirement content/status/revision. Duplicate/replayed
completion and failure events SHALL not repeat projections.

#### Scenario: Failure before assessment leaves business truth intact

- **WHEN** the runtime fails before producing an assessment
- **THEN** the server persists the operational failure fact and the Requirement's status, revision, and state_version remain unchanged

#### Scenario: Assessment and completion replay safely

- **WHEN** an assessment and completion are replayed with their original event identities
- **THEN** the assessment transaction and completion projection each apply at most once, with their existing ACK outcomes returned for duplicates

### Requirement: Availability is not Requirement failure

If no eligible daemon is connected, the server SHALL return an explicit
operational-unavailable result for a new clarification start and SHALL NOT
fabricate a runtime event or mark the Requirement failed. An existing pinned
run whose daemon is offline SHALL remain pinned and retain durable commands for
replay; it SHALL not migrate automatically. This change SHALL expose only
coarse `starting`, `running`, `completed`, or `unavailable` session status plus
cancellation intent. It SHALL NOT add `Idle`/`Retrying`/final `Failed` retry
policy, attempt accounting, retry budget, server backoff, or automatic
`session.resume`; those belong to
`introduce-runtime-retry-and-failure-state`.

#### Scenario: No daemon cannot corrupt lifecycle

- **WHEN** a requester starts or continues clarification while no eligible daemon is connected
- **THEN** the operation reports unavailability, preserves any durably posted message and current Requirement state except for an explicit valid Draft → Discussing transition, and does not mark the Requirement failed

#### Scenario: Offline owner is not silently replaced

- **WHEN** the pinned daemon disconnects during a run
- **THEN** the run remains owned by that daemon, pending delivery stays replayable, and no other daemon is selected by this change

### Requirement: Canonical read models are server-owned

The server SHALL provide canonical HTTP reads for the data needed by later
browser UI without requiring daemon traffic or SSE replay:

- existing Requirement and conversation reads, including persisted agent
  messages;
- `GET /requirements/{requirement_id}/readiness` for the latest immutable
  assessment, outcome/rejection reason, repository IDs/full SHAs, and a
  `current` flag tied to current revision/Ready generation;
- `GET /requirements/{requirement_id}/activity` for persisted coarse summaries;
  and
- `GET /requirements/{requirement_id}/session` for minimal session/runtime
  status and cancellation intent.

Missing assessment/history SHALL be represented as empty data, not inferred from
transport absence. The existing Ready-only review-packet projection remains
separate.

#### Scenario: Browser reads persisted agent output

- **WHEN** a browser refetches conversation after reconnect
- **THEN** it receives server-persisted requester and agent messages without reading daemon frames

#### Scenario: Current assessment is explicit

- **WHEN** the latest assessment targets an old revision or old Ready generation
- **THEN** the readiness read identifies it as historical/non-current rather than presenting it as current truth

### Requirement: Browser SSE is notification-only and server-produced

`north-server` SHALL own one authenticated SSE producer/endpoint for board and
detail invalidation. After a canonical transaction commits, it MAY emit
lightweight `requirement.changed`, `conversation.changed`,
`readiness.changed`, `activity.changed`, or `session.changed` notifications
containing requirement identity and non-authoritative metadata only. SSE SHALL
not be a durable browser event log, Requirement source of truth, WebSocket, or
required replay mechanism. Missed, duplicate, or reconnect-delivered hints SHALL
be harmless because clients refetch canonical HTTP reads.

#### Scenario: Missed hint is repaired by HTTP

- **WHEN** a browser misses an activity or Requirement notification while disconnected
- **THEN** reconnect/refocus refetch returns current server state without replaying an SSE history

### Requirement: Cancellation is one idempotent durable command

A cancellation request SHALL create or reuse one durable `session.cancel`
command for the pinned run. Repeated requests and command replays SHALL stop
runtime cancellation at most once and SHALL not append duplicate messages or
change Requirement state. Cancellation remains a run fact; it does not start a
retry or decide final execution failure.

#### Scenario: Repeated cancel is harmless

- **WHEN** a requester cancels an active run and the cancel command is retried
- **THEN** the daemon receives one logical cancellation operation and prior conversation/Requirement state remains intact
