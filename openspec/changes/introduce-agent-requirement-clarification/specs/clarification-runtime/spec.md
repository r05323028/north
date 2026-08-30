## Purpose

Defines North 0.1 server-authorized clarification executions through the
existing North protocol and durable delivery seams, with Pi Agent as the
reference runtime adapter and first end-to-end vertical slice. It persists safe
runtime facts and applies readiness without moving business authority to the
daemon.

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

### Requirement: Server assembles and pins each run context

A valid `clarification/start` MAY resolve the latest run only for its sequential
create/reuse decision because the operation creates or returns the run identity.
The latest run MAY be reused only when all of these are true: `daemon_id = null`;
no `session.start` was successfully created or dispatched; it has not been
cancelled or closed; the request is the same logical start attempt; and the
incoming `message_id` equals its recorded `start_message_id`. In this case the
server SHALL reuse the run identity and attempt daemon selection again. A
different message while that unassigned attempt is reusable SHALL return the
canonical conflict.

If the latest run is assigned and active (`starting` or `running`, including an
assigned run that is operationally unavailable because its daemon disconnected),
a different start message SHALL return the canonical conflict and SHALL create
no second run or `session.start` command. Later requester messages use the
explicit run-scoped dispatch operation. If the latest run is completed, cancelled,
or otherwise explicitly terminal/inapplicable for start reuse, a new persisted
eligible start message SHALL create a new run identity rather than retargeting
the prior run.

When a daemon is selected for a new or reused run, the server SHALL persist the
run's immutable `daemon_id`, bind it to one Requirement, persist the run
repository IDs, and atomically persist the complete `session.start` command
before dispatch. A new run SHALL receive the current Requirement snapshot and
revision, its own `run_id`/`session_id` identity, `start_message_id`, repository
set, and command/event sequence. `session.start` SHALL contain the immutable
Requirement snapshot, a deterministic bounded excerpt selected by North from
canonical persisted conversation history, and enabled repository metadata only.
The excerpt SHALL always include `start_message_id`; North's fixed configured
selection/truncation policy is authoritative. The envelope's `session_id` is the
same identity as application `run_id`; the payload SHALL NOT gain a duplicate
session field. Credentials, checkout paths, database handles, and
`north-domain` types SHALL NOT cross the daemon boundary. A run's repository set,
start message, selected context, and snapshot SHALL NOT be silently retargeted on
later command replay. Any user-driven Draft → Discussing transition used by
start orchestration SHALL honor `expected_state_version`; `revision` is the
content snapshot token, not the write precondition.

#### Scenario: Start sees server context, not storage

- **WHEN** the server dispatches `session.start`
- **THEN** the daemon receives the server-selected Requirement, deterministic conversation excerpt, and repository context, with no credential or persistence access

#### Scenario: Start message is always retained

- **WHEN** North selects a bounded `session.start` conversation excerpt for a run
- **THEN** the excerpt contains the persisted message identified by `start_message_id`, even when older context must be removed to satisfy the bound

#### Scenario: Oldest context is deterministically truncated

- **GIVEN** canonical conversation history exceeds the configured context bound
- **WHEN** North assembles `session.start`
- **THEN** it retains the newest messages that fit, removes the oldest retained non-start messages first, always retains `start_message_id`, and emits retained messages in canonical persisted order

#### Scenario: Run context replay is stable

- **GIVEN** an immutable run has a persisted North-selected `session.start` context
- **WHEN** the server replays or reconstructs that run with the same canonical state and context configuration
- **THEN** it sends the same selected conversation excerpt and `start_message_id` without provider-dependent reselection

#### Scenario: Provider cannot select canonical context

- **WHEN** Pi or another runtime provider receives a run context
- **THEN** it processes the North-selected excerpt and cannot choose canonical persisted messages using provider-specific relevance logic

#### Scenario: Reconnect preserves pin and snapshot

- **WHEN** the pinned daemon reconnects after the start command was outboxed
- **THEN** the server replays the stored command to that same daemon with its original `run_id`/`session_id`, sequence, selected context, and payload

#### Scenario: Stale discussion start creates no new command

- **WHEN** an initial-message start uses an older `expected_state_version`
- **THEN** the server returns the canonical conflict, leaves Requirement state unchanged, keeps the already-persisted message, creates no new run or daemon command, and leaves any existing run unchanged

### Requirement: Authenticated HTTP mutations separate history from execution intent

The server SHALL expose these authenticated application operations without
introducing a generic command API:

- `POST /requirements/{requirement_id}/conversation/messages` accepts the
  existing requester message body, persists one canonical conversation message,
  and returns its `message_id`. It SHALL not start a run, select a daemon, or
  create a runtime command.
- `POST /requirements/{requirement_id}/clarification/start` accepts
  `{ "message_id": string, "expected_state_version": u64 }`. It SHALL verify
  that the message belongs to this Requirement's conversation and is eligible
  as the start message, apply the canonical Draft → Discussing operation when
  required, and resolve the latest run only for sequential create/reuse. A
  reusable unassigned attempt requires `daemon_id = null`, no successfully
  created or dispatched `session.start`, no cancellation/closure, the same
  logical start attempt, and the recorded `start_message_id`; that request
  reuses the run. A different message while that attempt remains reusable
  returns the canonical conflict. An assigned active run also rejects a
  different start message. After a completed, cancelled, or otherwise terminal/
  inapplicable run, a new eligible persisted message creates a new run with a
  new `run_id` and current Requirement snapshot. An assigned start returns `202`
  with canonical Requirement/run data including `run_id`; no eligible daemon
  returns `503 clarification_unavailable` with the unassigned run projection. A
  stale `expected_state_version` returns the canonical `409` conflict before
  any run/command mutation, while preserving the already-persisted message.
- `POST /requirements/{requirement_id}/clarification/runs/{run_id}/messages/{message_id}/dispatch`
  has no message body. It SHALL verify that `run_id` exists, belongs to the
  Requirement in the URL, is assigned and active (`starting` or `running`,
  including pinned operational unavailability), and that the persisted requester
  message belongs to this Requirement's canonical conversation, is eligible for
  that run, and is not the recorded start message. It then creates or reuses exactly one durable `message.send` command
  for that explicit run and message. A pinned offline daemon retains the
  command for replay; an unassigned run returns `503 clarification_unavailable`
  without a `message.send` command. It SHALL never create a second conversation
  message or resolve a newer latest run.
- `POST /requirements/{requirement_id}/clarification/runs/{run_id}/cancel` has
  no message body. It SHALL verify that `run_id` exists, belongs to the
  Requirement in the URL, and is an unassigned not-yet-started run or an
  assigned active run (`starting` or `running`, including pinned operational
  unavailability); repeated cancellation of the same run remains idempotent. It
  SHALL persist `cancel_requested` and return that run's public projection. If the run is
  assigned, it SHALL also create or reuse exactly one durable `session.cancel`
  command for the pinned daemon. If the run is unassigned, cancellation is
  server-owned run state only: it SHALL create no `session.cancel` command and
  no command identity. Repeated cancellation returns that run's persisted
  state. With no run it returns `404 clarification_not_started`. Cancellation
  SHALL not mutate Requirement lifecycle, content, revision, or state_version.

`clarification/start` is the only identity-creating exception and returns the
public `run_id`. After a run ID is known, dispatch and cancellation SHALL always
include it explicitly. The latest-run `GET /requirements/{requirement_id}/session`
read may guide UI presentation but MUST NOT determine mutation identity. A stale
client targeting run A after run B becomes latest is evaluated only against A;
it is never silently retargeted to B. An ineligible target returns its
run-scoped canonical conflict or persisted idempotent result without creating a
command for another run.

#### Scenario: Posting history does not invoke runtime

- **WHEN** a requester posts to `/conversation/messages`
- **THEN** the message row commits and returns its identity without creating a run or daemon command

#### Scenario: Start uses persisted message and expected state

- **WHEN** a requester posts message M and calls `/clarification/start` with M's ID and the current expected_state_version
- **THEN** the server applies any valid Draft → Discussing transition, creates/reuses the run, returns its `run_id`, includes M in assigned `session.start` context, and creates no `message.send` for M

#### Scenario: Stale start preserves the message

- **WHEN** `/clarification/start` receives an older expected_state_version
- **THEN** the server returns canonical `409`, preserves M, creates no new run or daemon command, and leaves any existing run unchanged

#### Scenario: Later dispatch reuses one command for its run

- **WHEN** a persisted later message M is dispatched more than once to explicit run A
- **THEN** all requests use `/clarification/runs/A/messages/M/dispatch`, reuse one message-to-command mapping, and the runtime receives at most one logical `message.send`; dispatching the recorded start message is rejected and cannot create a second command

#### Scenario: Reuse unavailable start attempt

- **WHEN** run A is unassigned and unavailable, has never created or dispatched `session.start`, is not cancelled, and `/clarification/start` is retried with A's recorded `start_message_id`
- **THEN** the server reuses A's run identity and attempts daemon selection again without creating a competing run

#### Scenario: Different message cannot replace reusable attempt

- **WHEN** an unassigned reusable run A exists and `/clarification/start` references a different persisted message
- **THEN** the server returns the canonical conflict and creates no new run or `session.start` command

#### Scenario: Active assigned run rejects concurrent start

- **WHEN** run A is assigned and `starting` or `running` and a requester attempts `/clarification/start` with another persisted message
- **THEN** the server returns the canonical conflict and creates no second run or `session.start` command

#### Scenario: New run after completion

- **WHEN** clarification run A is completed, requester persists eligible message M2, and calls `/clarification/start` with M2 and the current `expected_state_version`
- **THEN** the server creates run B with a new `run_id` and current Requirement snapshot, while run A remains immutable historical data

#### Scenario: Cancel before daemon assignment

- **WHEN** an unassigned unavailable run A is cancelled through `/clarification/runs/A/cancel`
- **THEN** `cancel_requested` becomes true, A is ineligible for reuse, no `session.cancel` command or command identity exists, and Requirement state remains unchanged

#### Scenario: Repeated unassigned cancellation is state-only

- **WHEN** cancellation is requested repeatedly through `/clarification/runs/A/cancel` for an unassigned run A
- **THEN** the server returns the same persisted cancellation state and creates no daemon command

#### Scenario: Assigned cancellation uses pinned command

- **WHEN** an assigned active run A is cancelled through `/clarification/runs/A/cancel`
- **THEN** the server creates or reuses exactly one durable `session.cancel` command for A's pinned daemon and does not invoke cancellation twice

#### Scenario: Stale dispatch cannot target a newer run

- **GIVEN** browser state references run A
- **AND** run A becomes terminal
- **AND** run B is subsequently created
- **WHEN** the stale browser sends a dispatch targeting run A
- **THEN** North evaluates only run A according to its current eligibility and MUST NOT mutate or dispatch against run B

#### Scenario: Stale cancellation cannot target a newer run

- **GIVEN** browser state references run A
- **AND** run A becomes terminal
- **AND** run B is subsequently created
- **WHEN** the stale browser sends a cancel targeting run A
- **THEN** North evaluates only run A according to its current eligibility and MUST NOT mutate or cancel run B

#### Scenario: New run after cancelled unassigned attempt

- **WHEN** run A was cancelled before daemon assignment, requester persists a new eligible message M2, and explicitly starts clarification
- **THEN** A remains historical and the server creates run B instead of reusing A

### Requirement: Requester messages are durable before runtime dispatch

For every requester message that is sent to a runtime, the server SHALL first
commit it through the canonical conversation persistence path and obtain its
stable message identity. Only then SHALL it create or reuse one durable
`message.send` command containing that identity and content. Dispatch/replay
SHALL use existing server outbox and daemon journal identity mapping. A
duplicate/replayed command SHALL submit the logical message to the runtime at
most once. If dispatch is unavailable after message commit, the message SHALL
remain in canonical history and the run SHALL report operational
unavailability separately.

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

### Requirement: Pi Agent is North 0.1 reference clarification runtime adapter

North 0.1 SHALL implement exactly one concrete clarification runtime adapter:
`PiClarificationAdapter` inside `north-daemon`, backed by Pi Agent. Pi Agent is
North's reference clarification runtime adapter and first end-to-end runtime
vertical slice. This is a deliberate validation slice, not a multi-provider
runtime framework or provider-selection feature.

The daemon SHALL invoke `PiClarificationAdapter` through a daemon-private,
North-owned `ClarificationRuntime` seam. The seam SHALL be defined from North's
clarification execution needs rather than copied from Pi Agent's API or
lifecycle, and SHALL remain provider-neutral. Its conceptual North-owned inputs
SHALL be limited to:

- stable operation identity;
- session/run identity;
- immutable Requirement snapshot;
- deterministic persisted conversation context;
- authorized, run-bound repository inspection handles/context; and
- cancellation/control intent.

Its conceptual North-owned outputs SHALL be limited to:

- agent message;
- coarse product-visible activity;
- readiness assessment;
- completion; and
- operational failure.

The seam SHALL NOT carry or expose:

- Pi SDK types;
- Pi event names;
- Pi session objects;
- Pi tool-call schemas;
- provider-specific lifecycle state;
- raw tool output;
- chain-of-thought or reasoning; or
- Pi-specific configuration structures.

All Pi-specific mapping SHALL remain inside `PiClarificationAdapter`. The
adapter SHALL translate Pi callbacks/results into existing North-neutral
runtime facts or drop details with no North meaning. `north-daemon` SHALL emit
only existing North protocol events; no Pi event SHALL be mirrored as a new
protocol frame. Pi-specific APIs, lifecycle concepts, configuration, event
types, and SDK types MUST remain confined to the adapter within
`north-daemon` and MUST NOT become North protocol, domain, persistence, server,
or browser concepts. SDK dependencies SHALL not appear in
`north-server`, `north-domain`, or `north-protocol`.

The daemon and adapter SHALL report runtime facts only. They SHALL NOT mutate
Requirement state, apply Requirement business transitions, or access server
persistence directly. `north-server` remains responsible for canonical
conversation, readiness, and session projections through North's existing
domain and persistence paths. The daemon SHALL retain only local transport
reconnect, journal recovery, and runtime reattachment authority. This change
introduces no provider registry, provider-selection API, or abstraction for a
hypothetical runtime; it defines only the smallest seam needed to support Pi
cleanly.

#### Scenario: Pi proves the runtime seam end to end

- **GIVEN** a connected daemon has `PiClarificationAdapter` configured
- **WHEN** North starts an authorized clarification run
- **THEN** the server dispatches the existing North `session.start` command and the run is routed through the generic North clarification-runtime seam
- **AND** Pi processes the server-assembled Requirement, conversation, and repository context
- **AND** Pi can inspect only repositories already authorized and bound to that run
- **AND** Pi output is translated into North-neutral runtime facts for agent message, coarse activity, readiness, completion, or operational failure
- **AND** the server persists canonical conversation, readiness, and session projections
- **AND** no Pi-specific type or lifecycle concept crosses the daemon's North-facing boundary

#### Scenario: Pi-specific events remain private

- **WHEN** Pi emits SDK/provider-specific callbacks, tool records, or lifecycle events
- **THEN** `PiClarificationAdapter` maps each relevant result to an existing North-neutral fact or drops it
- **AND** no new `north-protocol` frame is introduced merely to mirror a Pi event

#### Scenario: Runtime seam is replaceable

- **WHEN** another runtime implementation satisfies the North clarification-runtime contract
- **THEN** replacing `PiClarificationAdapter` does not require changing `north-domain`, the canonical Requirement/conversation/readiness models, or the server-daemon wire protocol

#### Scenario: Repository access remains North-authorized

- **WHEN** Pi needs repository context during clarification
- **THEN** it can use only repository inspection context/handles already authorized and bound to the run by North
- **AND** Pi cannot independently choose arbitrary repositories, credentials, checkout paths, or server persistence access

#### Scenario: Runtime facts do not grant business authority

- **WHEN** Pi or its adapter produces a readiness, completion, or failure result
- **THEN** the daemon reports it through an existing North-neutral typed protocol event and `north-server` applies any canonical projection through existing validation and domain/persistence paths
- **AND** the daemon and Pi cannot directly mutate Requirement state or canonical server projections

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

### Requirement: Clarification runs are sequential and never competing

A valid explicit clarification start MAY resolve the latest run only for its
sequential create/reuse decision. The latest run MAY be reused only when
`daemon_id = null`, no `session.start` was successfully created or dispatched,
it has not been cancelled or closed, the request is the same logical start
attempt, and the incoming message is its recorded `start_message_id`. A
different message while that attempt remains reusable SHALL return the canonical
conflict. An assigned active run (`starting` or `running`, including an assigned
run temporarily unavailable because its daemon disconnected) SHALL also reject a
different start message and SHALL create no second run or `session.start`
command. The start response SHALL include the public `run_id` of the created or
reused run.

If the latest run is completed, cancelled, or otherwise explicitly
terminal/inapplicable for start reuse, a new eligible persisted requester message
and explicit start SHALL create a new run with a new `run_id`, the current
Requirement snapshot/revision, its own `start_message_id`, repository set,
eventual daemon pin, and independent command/event sequence. The existing
protocol `session_id` is that same run identity (`session_id = run_id`). The
prior run SHALL remain immutable historical data. Transport unavailability
alone SHALL not be treated as permission to migrate or create a competing run.

If no eligible daemon is connected for a new or reused unassigned run, the run
SHALL exist with `daemon_id = null`, status `unavailable`, and no `session.start`
command. The operation SHALL return `503 clarification_unavailable` with that
run projection and SHALL NOT fabricate a runtime event, mark the Requirement
failed, consume a retry attempt, or select a daemon implicitly later. A later
selection for that same unassigned start attempt requires another explicit start.

Once a run has a `daemon_id`, that daemon pin is immutable for this change. If
the pinned daemon disconnects, the run remains pinned, durable commands remain
replayable, and the public session read reports `status=unavailable` until
existing reconnect/delivery recovery resumes. The daemon is never migrated.

All dispatch and cancellation operations are run-scoped and require their
explicit `run_id`; they SHALL not resolve the latest run. This change SHALL
expose only coarse `starting`, `running`, `completed`, or `unavailable` session
status plus cancellation intent. It SHALL NOT add `Idle`/`Retrying`/final
`Failed` retry policy, attempt accounting, retry budget, server backoff, or
automatic `session.resume`; those belong to
`introduce-runtime-retry-and-failure-state`.

#### Scenario: No daemon still creates a run

- **WHEN** a valid requester start finds no eligible daemon
- **THEN** the server returns `503 clarification_unavailable` with a run identity, `daemon_id = null` internally, status `unavailable`, no `session.start` command, and any explicit valid Draft → Discussing Requirement transition already committed

#### Scenario: Reuse unavailable start attempt

- **WHEN** run A is unassigned and unavailable, has never been dispatched or cancelled, and the requester retries `/clarification/start` with A's recorded `start_message_id`
- **THEN** the server reuses A's `run_id` and attempts daemon selection again without creating a competing run

#### Scenario: No concurrent run while active

- **WHEN** run A is assigned and `starting` or `running` and the requester starts with another persisted message
- **THEN** the server returns the canonical conflict and creates no second run or `session.start` command

#### Scenario: New run after completion

- **WHEN** run A is completed, the requester persists eligible message M2, and calls `/clarification/start` with M2 and the current `expected_state_version`
- **THEN** the server creates run B with a new `run_id` and current Requirement snapshot, while run A remains immutable history

#### Scenario: Cancelled run allows a new run

- **WHEN** run A is cancelled and the requester later persists a new eligible message M2 and explicitly starts clarification
- **THEN** the server creates run B instead of reusing A, and A remains historical data

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
- `GET /requirements/{requirement_id}/session` for the latest clarification run
  for this Requirement, ordered by creation time, including its public `run_id`,
  minimal `starting`/`running`/`completed`/`unavailable` status, and separate
  `cancel_requested`. It SHALL return `{ "session": null }` only when no run has
  ever existed. An unassigned no-daemon run returns that run as `unavailable`;
  an assigned/offline run returns the same pinned run as `unavailable`; a
  completed or cancelled run remains readable until a newer run exists. After
  Run B is created, it is the latest result while prior runs remain internal
  historical persistence. Daemon IDs and other unnecessary daemon details are
  not exposed. This latest-run read is a UI convenience and MUST NOT determine
  dispatch or cancellation identity.

Missing assessment/history SHALL be represented as empty data, not inferred from
transport absence. The existing Ready-only review-packet projection remains
separate.

#### Scenario: Browser reads persisted agent output

- **WHEN** a browser refetches conversation after reconnect
- **THEN** it receives server-persisted requester and agent messages without reading daemon frames

#### Scenario: Current assessment is explicit

- **WHEN** the latest assessment targets an old revision or old Ready generation
- **THEN** the readiness read identifies it as historical/non-current rather than presenting it as current truth

#### Scenario: Session read returns latest run semantics

- **WHEN** a requester reads `/requirements/{requirement_id}/session` after an unassigned, assigned/offline, completed, or cancelled run exists
- **THEN** the server returns that latest run projection; it returns `{ "session": null }` only before any run exists, and a newer sequential run replaces the prior run as the latest result without deleting history

### Requirement: Clarification extends Board-owned browser SSE

`introduce-requirement-board` SHALL own the single authenticated `GET /events`
SSE endpoint and base `requirement.changed` category. This change SHALL extend
that same producer after clarification canonical transactions with lightweight
`conversation.changed`, `readiness.changed`, `activity.changed`, and
`session.changed` notifications containing Requirement identity and
non-authoritative metadata only. It SHALL not create another endpoint, event
bus, browser event store, or WebSocket path. SSE SHALL not be a durable browser
event log, Requirement source of truth, or required replay mechanism.
`Last-Event-ID` SHALL not be required for correctness; missed, duplicate,
delayed, out-of-order, or reconnect-delivered hints SHALL be harmless because
clients refetch canonical HTTP reads.

#### Scenario: Clarification hint is repaired by HTTP

- **WHEN** a browser misses a conversation, readiness, activity, or session notification while disconnected
- **THEN** reconnect/refocus refetch returns current canonical state without replaying an SSE history

### Requirement: Cancellation distinguishes run state from daemon command

An authenticated requester cancellation SHALL use the explicit run-scoped route
`POST /requirements/{requirement_id}/clarification/runs/{run_id}/cancel`. The
server SHALL validate that `run_id` exists, belongs to the Requirement in the URL,
and is an unassigned not-yet-started run or an assigned active run (`starting` or
`running`, including pinned operational unavailability) before persisting
`cancel_requested`; repeated cancellation of the same run remains idempotent. It
SHALL not resolve or target the latest run implicitly. Cancellation SHALL not mutate
Requirement lifecycle, content, revision, or state_version. If the explicit run
is assigned (`daemon_id != null`), the server SHALL also create or reuse exactly
one durable `session.cancel` command for that pinned daemon. If it is unassigned
(`daemon_id = null`) and has never created/dispatched `session.start`, the
server SHALL create no `session.cancel` command and no command identity; the
persisted run cancellation state alone makes it ineligible for reuse. With no
matching run it SHALL return `404 clarification_not_started` (or the existing
run-not-found contract). Repeated cancellation of that same run returns its
persisted state. Assigned command replays SHALL invoke runtime cancellation at
most once; cancellation never migrates the run or decides retry or final
execution failure policy. An ineligible terminal target returns its run-scoped
canonical result without creating a command for another run.

#### Scenario: Cancel before daemon assignment

- **WHEN** an unassigned unavailable run A is cancelled through `/clarification/runs/A/cancel`
- **THEN** `cancel_requested` becomes true, A becomes ineligible for reuse, no `session.cancel` command or command identity exists, and Requirement state remains unchanged

#### Scenario: Repeated unassigned cancellation

- **WHEN** cancellation is requested repeatedly through `/clarification/runs/A/cancel` for unassigned run A
- **THEN** the server returns A's same persisted cancellation state and creates no daemon command

#### Scenario: Assigned cancellation

- **WHEN** an assigned active run A is cancelled through `/clarification/runs/A/cancel`
- **THEN** the server creates or reuses exactly one durable `session.cancel` command for A's pinned daemon and does not invoke runtime cancellation twice

#### Scenario: Stale cancellation cannot target a newer run

- **GIVEN** browser state references run A
- **AND** run A becomes terminal
- **AND** run B is subsequently created
- **WHEN** the stale browser sends a cancel targeting run A
- **THEN** North evaluates only run A according to its current eligibility and MUST NOT mutate or cancel run B

#### Scenario: New run after cancelled unassigned attempt

- **WHEN** run A was cancelled before daemon assignment, requester later persists a new eligible message M2, and explicitly starts clarification
- **THEN** run A remains historical and the server creates run B instead of reusing A
