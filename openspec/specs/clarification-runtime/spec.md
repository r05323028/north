# clarification-runtime Specification

## Purpose

Defines North 0.1 server-authorized clarification executions through the
existing North protocol and durable delivery seams, with Pi Agent as the
reference runtime adapter and first end-to-end vertical slice. It persists safe
runtime facts and applies readiness without moving business authority to the
daemon.

## Requirements

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
create/reuse/idempotency decision because the operation creates or returns
the run identity.
The latest run MAY be reused only when all of these are true:
`phase=awaiting_assignment`, `daemon_id = null`, no `session.start` was
successfully created or dispatched, it has not been cancelled or closed; the
request is the same logical start attempt; and the incoming `message_id` equals
its recorded `start_message_id`. In this case the
server SHALL reuse the run identity and attempt daemon selection again. A
different message while that unassigned attempt is reusable SHALL return the
canonical conflict. If a matching `start_message_id` already has a committed run identity for an
unclosed start attempt, concurrent or retried same-message starts SHALL resolve
to that run and its existing command identities: one serialized request may complete
conditional assignment for an awaiting run, but no request creates a second run
or performs a second assignment, lifecycle transition, or `session.start`. The
state-version precondition gates a new start mutation; it does not turn a
matching idempotent retry into a second mutation.

If the latest run is `phase=active` (assigned and non-terminal, including a
pinned daemon that is operationally unavailable or a run with cancellation
requested), a different start message SHALL return the canonical conflict and
SHALL create no second run or `session.start` command. Later requester messages
use the explicit run-scoped dispatch operation. If the latest run is
`phase=terminal`, a new persisted eligible start message SHALL create a new run
identity rather than retargeting the prior run.

When a daemon is selected for a new or reused run, assignment is valid only
while the run remains the authoritative non-terminal `phase=awaiting_assignment`
occupant and has not been cancelled or closed. The server SHALL atomically verify
that eligibility while persisting the run's immutable `daemon_id`, Requirement
binding, repository IDs, complete `session.start` command, `phase=active`, and
`status=starting`; if eligibility is lost before commit, assignment SHALL fail
without persisting a daemon pin, binding, context, or command and SHALL not
reactivate the run. The operation occupies the sequential clarification slot;
`session.started` is not required to acquire the sequential clarification slot. A new run SHALL
receive the current Requirement snapshot and
revision, its own `run_id`/`session_id` identity, `start_message_id`, repository
set, and command/event sequence. `session.start` SHALL contain the immutable
Requirement snapshot, a deterministic bounded excerpt selected by North from
canonical persisted conversation history, and enabled repository metadata only.
The excerpt SHALL always include `start_message_id`; North's fixed configured
selection/truncation policy is authoritative. For North 0.1, the context bound
and size accounting SHALL use a fixed message count and/or UTF-8 byte size.
Token-based accounting is deferred unless a later change defines a canonical
provider-independent tokenizer and tokenizer version as part of the selection
configuration; no Pi tokenizer or tokenizer abstraction is introduced here.
The envelope's `session_id` is the same identity as application `run_id`; the
payload SHALL NOT gain a duplicate session field. Credentials, checkout paths, database handles, and
`north-domain` types SHALL NOT cross the daemon boundary. A run's repository set,
start message, selected context, and snapshot SHALL NOT be silently retargeted on
later command replay. Any user-driven Draft → Discussing transition used by
start orchestration SHALL honor `expected_state_version`; `revision` is the
content snapshot token, not the write precondition.

#### Scenario: Start sees server context, not storage

- **WHEN** the server dispatches `session.start`
- **THEN** the daemon receives the server-selected Requirement, deterministic conversation excerpt, and repository context, with no credential or persistence access

#### Scenario: Assignment acquires the sequential clarification slot before runtime startup

- **GIVEN** run A is `phase=awaiting_assignment`, `status=unavailable`, and has no daemon pin
- **WHEN** North atomically selects daemon D, persists D's pin, the Requirement/run binding, immutable run context, and complete `session.start` command
- **THEN** A becomes `phase=active`, `status=starting`, and occupies the sequential clarification slot before dispatch
- **AND** another clarification start conflicts even though `session.started` has not arrived; that event only retains `phase=active` and changes status to `running`

#### Scenario: Concurrent same-message starts are idempotent

- **GIVEN** Requirement R has no non-terminal clarification run and persisted requester message M is eligible to start clarification
- **WHEN** two `/clarification/start` requests for R and M execute concurrently
- **THEN** server/persistence arbitration creates exactly one run identity and both requests resolve to that same `run_id` under the canonical idempotent/reuse contract, even if their response timing or status differs
- **AND** at most one daemon assignment and one durable `session.start` command identity is committed; if no daemon is available, one `phase=awaiting_assignment` run is returned, otherwise one `phase=active`, `status=starting` run exists
- **AND** no second non-terminal run occupies the sequential clarification slot

#### Scenario: Concurrent different-message starts have one winner

- **GIVEN** Requirement R has no non-terminal clarification run and persisted requester messages M1 and M2 are each individually eligible
- **WHEN** `/clarification/start(M1)` and `/clarification/start(M2)` execute concurrently
- **THEN** exactly one request establishes the sequential run and the other observes the occupied slot and receives the existing different-message conflict; the winner is not required to be deterministic
- **AND** exactly one non-terminal run exists and at most one durable `session.start` command identity is committed
- **AND** both M1 and M2 remain canonical conversation history; the losing message is not deleted, rolled back, automatically dispatched, or converted into another run

#### Scenario: Concurrent retries reuse one awaiting run

- **GIVEN** run A is `phase=awaiting_assignment` with `start_message_id=M1`, no daemon pin, and no `session.start`
- **WHEN** two `/clarification/start(M1)` retries execute concurrently
- **THEN** both requests reuse A, daemon selection is serialized, and at most one daemon assignment and one durable `session.start` command identity is committed
- **AND** no second awaiting or active run is created

#### Scenario: Different message cannot replace a concurrently retried awaiting run

- **GIVEN** run A is `phase=awaiting_assignment` with `start_message_id=M1` and persisted eligible message M2 is different
- **WHEN** `/clarification/start(M1)` retries concurrently with `/clarification/start(M2)`
- **THEN** A remains the sequential clarification slot occupant, M1 may reuse or assign A, and M2 receives the existing different-message conflict rather than replacing A
- **AND** M2 remains canonical conversation history and no second run or `session.start` command is created

#### Scenario: Assignment rechecks awaiting eligibility before commit

- **GIVEN** run A is the authoritative `phase=awaiting_assignment` occupant with no daemon pin or `session.start`
- **WHEN** A becomes terminal or otherwise ineligible before daemon assignment commits
- **THEN** assignment fails without persisting a daemon pin, Requirement/run binding, immutable context, or `session.start`, and it does not reactivate A or create a command

#### Scenario: Cancellation wins awaiting-assignment race

- **GIVEN** run A is `phase=awaiting_assignment` with no daemon pin or `session.start`
- **WHEN** a start retry for A's `start_message_id` races cancellation and cancellation commits first
- **THEN** A has `cancel_requested=true`, `phase=terminal`, `daemon_id=null`, no `session.start`, and no `session.cancel`; the retry observes terminal/ineligible A and cannot assign or reactivate it

#### Scenario: Assignment wins awaiting-assignment race

- **GIVEN** run A is `phase=awaiting_assignment` with no daemon pin or `session.start`
- **WHEN** assignment commits first while a cancellation request races it
- **THEN** the assignment atomically commits the daemon pin, Requirement/run binding, immutable context, durable `session.start`, `phase=active`, and `status=starting`; cancellation then observes assigned active A, persists `cancel_requested=true`, creates or reuses exactly one durable `session.cancel`, and leaves A active in the sequential clarification slot until `session.completed` or `session.failed`
- **AND** no hybrid terminal-and-assigned result is exposed

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

#### Scenario: Stale genuinely new start creates no command

- **GIVEN** no non-terminal clarification run occupies the Requirement's sequential slot
- **WHEN** an initial-message start uses an older `expected_state_version`
- **THEN** the server returns the canonical conflict, leaves Requirement state unchanged, keeps the already-persisted message, creates no new run or daemon command, and leaves any existing terminal history unchanged

#### Scenario: Matching concurrent start ignores its original stale token

- **GIVEN** Requirement R is `Draft` with `state_version=1`, and persisted message M is eligible
- **WHEN** A and B concurrently call `start(M, expected_state_version=1)` and A wins arbitration
- **THEN** A applies Draft → Discussing once, commits `state_version=2`, and creates run A
- **AND** B resolves as an idempotent reference to A's `run_id`, without a second lifecycle transition, run, assignment, or `session.start`
- **AND** B does not fail as a stale genuinely new start because its matching logical start is already committed

#### Scenario: Different concurrent start receives existing-run conflict

- **GIVEN** Requirement R is `Draft` with `state_version=1`, and persisted messages M1 and M2 are eligible
- **WHEN** A calls `start(M1, expected_state_version=1)` and B calls `start(M2, expected_state_version=1)` concurrently, and A wins arbitration
- **THEN** A creates the one new run and B receives the canonical existing-run/different-message sequential-slot conflict
- **AND** B creates no run, lifecycle mutation, daemon assignment, or `session.start`, despite its originally current token
- **AND** both M1 and M2 remain canonical conversation history

### Requirement: Authenticated HTTP mutations separate history from execution intent

The server SHALL expose these authenticated application operations without
introducing a generic command API:

- `POST /requirements/{requirement_id}/conversation/messages` accepts the
  existing requester message body, persists one canonical conversation message,
  and returns its `message_id`. It SHALL not start a run, select a daemon, or
  create a runtime command.
- `POST /requirements/{requirement_id}/clarification/start` accepts
  `{ "message_id": string, "expected_state_version": u64 }`. After normal
  authentication/authorization, it validates that the message belongs to this
  Requirement's conversation and is eligible as the start message, enters the
  per-Requirement sequential-slot arbitration, and inspects the authoritative
  non-terminal occupant. If an occupant's `start_message_id` matches, the
  request is a same logical start retry: it reuses that `run_id` and existing
  command identities, creates no run, does not reapply Draft → Discussing, and
  does not reapply the original `expected_state_version` as a new-mutation
  precondition. If the message differs, it returns the canonical existing-run
  sequential-slot conflict, preserves the persisted message, and creates no
  Requirement mutation, run, daemon assignment, or `session.start`. Only an
  unoccupied slot is a genuinely new logical start. For that new start, the
  server atomically validates `expected_state_version`; a stale token returns
  canonical `409` before any new-run or command mutation while preserving the
  already-persisted message. A current token applies Draft → Discussing when
  required, then creates a new run with a new `run_id` and current Requirement
  snapshot. An assigned start returns `202` with canonical Requirement/run data
  including `run_id` and `start_message_id`; no eligible daemon returns `503
  clarification_unavailable` with the unassigned `phase=awaiting_assignment`,
  `status=unavailable` run projection. Sequential-slot arbitration therefore
  decides reuse, different-message conflict, or genuinely new start before the
  new-start state-version precondition is applied. The state version protects
  creation of a new logical clarification start and its associated Requirement
  transition; it does not invalidate an already-committed matching logical start
  during idempotent replay or concurrent same-message arbitration.
- `POST /requirements/{requirement_id}/clarification/runs/{run_id}/messages/{message_id}/dispatch`
  has no message body. It SHALL verify that `run_id` exists, belongs to the
  Requirement in the URL, is `phase=active` with `cancel_requested=false` and
  an assigned non-terminal run (including a pinned daemon that is operationally
  unavailable), and that the
  persisted requester message belongs to this Requirement's canonical
  conversation, is eligible for that run, and is not the recorded start message.
  It then creates or reuses exactly one durable `message.send` command for that
  explicit run and message. An `awaiting_assignment` or terminal run returns its
  run-scoped conflict/unavailability result without a command. A pinned offline
  daemon retains the command for replay. It SHALL never create a second
  conversation message or resolve a newer latest run.
- `POST /requirements/{requirement_id}/clarification/runs/{run_id}/cancel` has
  no message body. It SHALL verify that `run_id` exists, belongs to the
  Requirement in the URL, and is an unassigned not-yet-started run or an
  assigned `phase=active` run with a non-terminal execution (including pinned
  operational unavailability); repeated cancellation of the same run remains
  idempotent. It SHALL persist `cancel_requested` and return that run's public
  projection. If the run is unassigned with no `session.start` execution, it
  immediately sets `phase=terminal`, creates no `session.cancel` or other daemon
  command identity, and makes the run ineligible for reuse. If the run is
  assigned, it creates or reuses exactly one durable `session.cancel` for the
  pinned daemon but remains `phase=active` and continues to occupy the
  sequential clarification slot until `session.completed` or `session.failed` is durably
  projected. A `command_ack` only means the daemon recorded the command; it is
  not cancellation completion. Cancellation SHALL not mutate Requirement
  lifecycle, content, revision, or state_version.

`clarification/start` is the only identity-creating exception and returns the
public `run_id` and `start_message_id`. After a run ID is known, dispatch and
cancellation SHALL always include it explicitly. The latest-run
`GET /requirements/{requirement_id}/session` read may guide UI presentation but
MUST NOT determine mutation identity. A stale client targeting run A after run B
becomes latest is evaluated only against A; it is never silently retargeted to B.
An unknown or Requirement-mismatched run ID on an explicit run-scoped route
returns HTTP `404` with generic error code `not_found` after normal
authorization checks, without revealing cross-Requirement run existence. An
ineligible target returns its run-scoped canonical conflict or persisted
idempotent result without creating a command for another run.

#### Scenario: Posting history does not invoke runtime

- **WHEN** a requester posts to `/conversation/messages`
- **THEN** the message row commits and returns its identity without creating a run or daemon command

#### Scenario: Start uses persisted message and expected state

- **WHEN** a requester posts message M and calls `/clarification/start` with M's ID and the current expected_state_version
- **THEN** the server applies any valid Draft → Discussing transition, creates/reuses the run, returns its `run_id`, includes M in assigned `session.start` context, and creates no `message.send` for M

#### Scenario: Stale new start preserves the message

- **GIVEN** no non-terminal clarification run occupies the Requirement's sequential slot
- **WHEN** `/clarification/start` receives an older `expected_state_version` for persisted message M
- **THEN** the server returns canonical `409`, preserves M, creates no new run, daemon assignment, or daemon command, and leaves terminal history unchanged

#### Scenario: Same-message Draft retry is not stale after the winner transitions

- **GIVEN** Requirement R is `Draft` with `state_version=1`, and A and B both start persisted message M with `expected_state_version=1`
- **WHEN** A wins arbitration, applies Draft → Discussing, commits `state_version=2`, and creates run A before B resolves
- **THEN** B reuses A's logical start and returns A's `run_id` without another transition or run
- **AND** B's original `expected_state_version=1` is not applied as a new-start precondition

#### Scenario: Different message loses to the existing run

- **GIVEN** A and B start different persisted messages with the same current expected state version and A wins the empty slot
- **WHEN** B reaches sequential-slot arbitration
- **THEN** B receives the canonical existing-run/different-message conflict, creates no run or Requirement mutation, and its persisted message remains history

#### Scenario: Later dispatch reuses one command for its run

- **WHEN** a persisted later message M is dispatched more than once to explicit run A
- **THEN** all requests use `/clarification/runs/A/messages/M/dispatch`, reuse one message-to-command mapping, and the runtime receives at most one logical `message.send`; dispatching the recorded start message is rejected and cannot create a second command

#### Scenario: Cancellation-pending active run rejects later dispatch

- **GIVEN** assigned run A is `phase=active` with `cancel_requested=true`
- **WHEN** a later persisted message is dispatched to A
- **THEN** dispatch fails/conflicts, creates no `message.send` command, and A remains active in the sequential clarification slot; repeated cancellation remains idempotent

#### Scenario: Persisted message survives cancellation/dispatch race

- **GIVEN** requester message M is durably persisted for assigned active run A
- **WHEN** cancellation for A commits `cancel_requested=true` before dispatch of M
- **THEN** M remains canonical conversation history, dispatch fails/conflicts, no `message.send` command is created, and North does not delete or roll back M

#### Scenario: Reuse unavailable start attempt

- **WHEN** run A is `phase=awaiting_assignment`, has never created or dispatched `session.start`, is not cancelled, and `/clarification/start` is retried with A's public `start_message_id`
- **THEN** the server reuses A's run identity and attempts daemon selection again without creating a second non-terminal run

#### Scenario: Different message cannot replace reusable attempt

- **WHEN** an unassigned reusable run A exists and `/clarification/start` references a different persisted message
- **THEN** the server returns the canonical conflict and creates no new run or `session.start` command

#### Scenario: Active phase rejects concurrent start

- **WHEN** run A is `phase=active` and a requester attempts `/clarification/start` with another persisted message, including after `session.cancel` receives `command_ack`
- **THEN** the server returns the canonical conflict, creates no second run or `session.start` command, and keeps A in the sequential clarification slot until terminal runtime projection

#### Scenario: New run after completion

- **WHEN** clarification run A is completed, requester persists eligible message M2, and calls `/clarification/start` with M2 and the current `expected_state_version`
- **THEN** the server creates run B with a new `run_id` and current Requirement snapshot, while run A remains immutable historical data

#### Scenario: Unassigned cancellation is immediately terminal

- **WHEN** run A is `phase=awaiting_assignment` with no `session.start` or runtime execution and is cancelled through `/clarification/runs/A/cancel`
- **THEN** `cancel_requested` becomes true, A becomes `phase=terminal` and ineligible for reuse, no `session.cancel` command or command identity exists, and Requirement state remains unchanged

#### Scenario: Repeated unassigned cancellation is state-only

- **WHEN** cancellation is requested repeatedly through `/clarification/runs/A/cancel` for an unassigned run A
- **THEN** the server returns the same persisted cancellation state and creates no daemon command

#### Scenario: Assigned cancellation remains active after acknowledgement

- **WHEN** an assigned `phase=active` run A is cancelled through `/clarification/runs/A/cancel` and its `session.cancel` receives `command_ack`
- **THEN** the server creates or reuses exactly one durable `session.cancel` for A, leaves A `phase=active`, and does not permit a new run until `session.completed` or `session.failed` is durably projected

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

For well-formed session-bound runtime events, North SHALL retain existing event
identity/sequence validation and ACK only after one idempotent server
projection commits. `session.failed` is an execution-attempt fact, not an
unconditionally terminal logical-run fact. A known failure with retry policy
budget remaining clears/closes the failed current attempt, persists safe failure
classification and `next_retry_at`, and leaves the logical run
`phase=active,status=retrying`. Exhaustion, `execution_outcome_unknown`,
owner invalidation, or cancellation terminalizes the logical run as
`phase=terminal,status=failed`. A retrying run retains the sequential slot; a
terminal run releases it. Raw runtime/provider details remain private.

`session.completed` remains successful terminal completion. Duplicate/replayed
facts return their recorded ACK/outcome without repeating effects. A terminal
unknown-outcome run can never receive `session.resume`; later execution starts a
new logical run with a new run/protocol session identity and normal start/slot
rules.

#### Scenario: Agent message becomes canonical history

- **WHEN** a valid `agent.message` event commits
- **THEN** one canonical agent message is persisted and duplicate delivery does
  not add another message

#### Scenario: Completion does not mean Ready

- **WHEN** `session.completed` arrives without an accepted assessment
- **THEN** the run completes without changing Requirement lifecycle or creating
  synthetic readiness

#### Scenario: Retryable failure remains active

- **WHEN** a known current-attempt `session.failed` fact is accepted with retry
  budget remaining
- **THEN** the failed current attempt is closed, the run is active/retrying with
  durable due work, and no new logical run is created

#### Scenario: Unknown outcome forbids same-run resume

- **WHEN** `execution_outcome_unknown` terminalizes run A
- **THEN** A cannot receive `session.resume`; later execution requires new run B,
  new `run_id`/protocol `session_id`, and a new `session.start`

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

A normal completion SHALL remain a valid terminal session fact even without an
assessment and never changes Requirement content, lifecycle, revision, readiness,
or state version. A failure fact is processed once and then follows server
retry policy. Known retryable failure keeps the logical run active and
slot-occupying; retry exhaustion, unknown outcome, invalid owner, or terminal
cancellation failure
produces safe terminal failed state. Successful cancellation uses the existing
completion fact. The daemon `recoverable` value is a local fact only.

#### Scenario: Failure before assessment leaves business truth intact

- **WHEN** runtime fails before producing an assessment
- **THEN** server policy shows active/retrying or terminal/failed without changing
  Requirement status, revision, readiness, or state version

#### Scenario: Successful cancellation uses existing completion fact

- **WHEN** a requested runtime cancellation terminates successfully
- **THEN** the existing `session.completed` fact projects terminal/completed with
  cancellation intent and no new cancellation frame

#### Scenario: Assessment and completion replay safely

- **WHEN** assessment and completion facts replay with original identities
- **THEN** each committed effect applies once and duplicate ACK outcomes remain
  stable

### Requirement: Clarification runs occupy one sequential clarification slot

The derived slot SHALL remain occupied by `phase=awaiting_assignment` and every
assigned non-terminal run, including `phase=active,status=retrying`. A terminal
failed, completed, or explicitly cancelled run releases it. A retry worker may
create a new attempt only for the same non-terminal run while it is retry-eligible.
A terminal run, especially one terminalized by unknown outcome, is never resumed
and cannot block or mutate a newer run.

#### Scenario: No daemon still creates an awaiting run

- **WHEN** no eligible daemon exists for a new start
- **THEN** the unassigned awaiting run occupies the slot without an execution
  attempt

#### Scenario: Reuse unavailable start attempt

- **WHEN** the same start message retries an awaiting run
- **THEN** the server reuses that run and does not create another run or start
  command

#### Scenario: No concurrent run while active

- **WHEN** run A is active/retrying and another start message arrives
- **THEN** the request receives the existing-run conflict and creates no run or
  start command

#### Scenario: Assigned cancellation intent does not terminate execution

- **WHEN** cancellation is acknowledged for a running assigned attempt
- **THEN** the run remains active until its terminal runtime fact and no later
  failure can schedule retry after cancellation intent

#### Scenario: Terminal runtime fact releases cancelled run

- **WHEN** a cancelled running attempt reaches terminal completion/failure
- **THEN** the run becomes terminal, releases the slot, and a later start may
  create a new logical run

#### Scenario: Unassigned cancellation is immediately terminal

- **WHEN** an awaiting run with no start command is cancelled
- **THEN** it becomes terminal without daemon command or execution attempt

#### Scenario: New run after terminal completion

- **WHEN** run A is terminal and a new eligible message starts clarification
- **THEN** run B has a new identity and independent command/event sequence

#### Scenario: Retry-waiting cancellation releases slot

- **WHEN** a retrying run with no current attempt is cancelled before due resume
- **THEN** server clears due work, terminalizes the run safely, and a stale worker
  cannot create or resurrect it

### Requirement: Canonical read models are server-owned

The existing session read SHALL remain the only public run projection. It retains
`awaiting_assignment`/`active`/`terminal` phases and adds safe
`attempt_count`, nullable `next_retry_at`, and bounded `failure_reason` as
available. A policy retry reads `phase=active,status=retrying`; terminal
execution failure, cancellation, owner invalidation, or unknown outcome reads
`phase=terminal,status=failed`. The projection never exposes retry limits,
remaining budget, daemon/provider identity, raw runtime errors, credentials, or
operation IDs.

#### Scenario: Browser reads persisted agent output

- **WHEN** browser refetches conversation after reconnect
- **THEN** it receives persisted messages without reading daemon frames

#### Scenario: Current assessment is explicit

- **WHEN** an assessment targets an old revision or Ready generation
- **THEN** readiness read marks it historical/non-current

#### Scenario: Session read returns latest run semantics

- **WHEN** a requester reads after an awaiting, running, retrying, completed,
  failed, or cancelled run exists
- **THEN** the latest run projection is returned, while old run history remains
  server-owned and a new start uses a new run identity

#### Scenario: Retry projection retains the sequential slot

- **WHEN** a known execution failure is retryable and the server persists a due retry
- **THEN** the session read returns `phase=active`, `status=retrying`, safe retry
  fields only, and a new start cannot claim the Requirement slot

#### Scenario: Terminal failure releases the slot

- **WHEN** retry policy exhausts or rejects an unknown execution outcome
- **THEN** the session read returns `phase=terminal`, `status=failed`, the
  Requirement remains unchanged, and a later eligible start may create a new run

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

### Requirement: Cancellation distinguishes intent from completion

Cancellation SHALL remain explicit and run-scoped. Running attempts persist intent
and create/reuse one pinned `session.cancel`; later failure cannot schedule retry.
Retrying/waiting runs with no current attempt terminalize locally, clear due work,
and release the slot. Cancellation, worker, reconnect, and stale old-run
operations cannot resurrect a terminal run or affect a newer run.

#### Scenario: Unassigned cancellation is immediately terminal

- **WHEN** an unassigned run is cancelled
- **THEN** cancellation is persisted terminally without a daemon command

#### Scenario: Repeated unassigned cancellation

- **WHEN** the same unassigned run is cancelled repeatedly
- **THEN** persisted terminal state is reused idempotently

#### Scenario: Cancellation intent does not terminate assigned execution

- **WHEN** assigned cancellation receives `command_ack` but no runtime terminal
  fact exists
- **THEN** the run remains active and slot-occupying, with later dispatch blocked

#### Scenario: Terminal runtime fact releases cancelled run

- **WHEN** a cancelled assigned attempt emits terminal fact
- **THEN** the server closes that run according to completion/failure policy and
  releases its slot

#### Scenario: Successful cancellation uses existing completion fact

- **WHEN** runtime confirms successful cancellation
- **THEN** `session.completed` projects terminal/completed and preserves intent

#### Scenario: Stale cancellation cannot target a newer run

- **WHEN** delayed cancellation targets terminal run A after run B exists
- **THEN** only A is evaluated and B is untouched

#### Scenario: Unknown or cross-Requirement run is not found

- **WHEN** cancellation names an unknown or cross-Requirement run
- **THEN** server returns not-found and creates no command or state change
