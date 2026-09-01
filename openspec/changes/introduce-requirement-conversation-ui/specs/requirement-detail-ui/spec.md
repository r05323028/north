## Purpose

Renders a Requirement detail surface from canonical server reads: persisted
dialogue, structured Requirement state, current readiness evidence, coarse
activity, and minimal clarification status.

## ADDED Requirements

### Requirement: Detail uses canonical HTTP read models

The Board-owned `/requirements/[id]` route is the base Requirement detail shell.
This change SHALL extend that existing route rather than create or take ownership
of another detail route. The detail page SHALL read Requirement, conversation,
latest/current readiness, coarse activity, and the public session projection
from authenticated HTTP APIs. It SHALL not read daemon WebSocket traffic,
interpret protocol events as product truth, or reconstruct any model from an SSE
replay log. The page SHALL offer Conversation, Overview, and Activity tabs.

#### Scenario: Refetch restores the complete detail bundle

- **WHEN** the detail page loads or regains focus
- **THEN** it refetches the Requirement, conversation, readiness, activity, and session reads and renders each from its corresponding canonical response

### Requirement: Conversation persistence and clarification intent are separate

Conversation SHALL render requester and agent messages returned by the canonical
conversation API. `POST /requirements/{requirement_id}/conversation/messages`
SHALL persist one requester message and return its `message_id`; it SHALL not
infer runtime intent, start a run, choose a daemon, or create
`session.start`/`message.send`.

For an initial clarification message, the UI SHALL call the identity-creating
`POST /requirements/{requirement_id}/clarification/start` with the persisted
`message_id` and current `expected_state_version`; the response includes the
public `run_id` and `start_message_id`. For a later message, it SHALL use the
known `run_id` from that start response or the explicit `run_id` exposed by the
canonical session read and call
`POST /requirements/{requirement_id}/clarification/runs/{run_id}/messages/{message_id}/dispatch`.
The URL's explicit `run_id`, not the read's latestness, determines the mutation
target. The initial message SHALL be represented in `session.start` context and
SHALL not be submitted again as `message.send`. The cancel control SHALL use a
known `run_id` and call
`POST /requirements/{requirement_id}/clarification/runs/{run_id}/cancel`.

After persistence, the latest `GET /requirements/{requirement_id}/session` read
may guide presentation and supplies the public run projection, but it SHALL NOT
implicitly determine mutation identity. The UI SHALL use `phase`, not coarse
`status` alone, to choose legal intent:

- no run (`session: null`) starts clarification;
- `phase=awaiting_assignment` allows same-start retry only with the canonical
  public `start_message_id`, or cancellation of that explicit `run_id`; it does
  not dispatch later messages or create a competing start;
- `phase=active` with `cancel_requested=false` allows later-message dispatch
  and cancellation to the explicit `run_id`, but never a competing start, even
  when `status=unavailable` because the pinned daemon is disconnected;
- `phase=active` with `cancel_requested=true` keeps the run competing and
  permits only idempotent cancellation; later-message dispatch is rejected; and
- `phase=terminal` allows a new persisted eligible message to use `start`.

Except for identity-creating `start`, the UI SHALL never perform dispatch or
cancellation without a known `run_id`; a stale known ID remains in the URL and
is never replaced with a newer latest-run ID. A different message during an
awaiting-assignment attempt or active run is a server conflict, not a local
run. The UI never infers the operation solely from transcript contents or
selects canonical context messages. Duplicate/replayed command delivery SHALL
not add another logical message or runtime submission.

#### Scenario: Posting history does not invoke runtime

- **WHEN** a requester posts `POST /requirements/{requirement_id}/conversation/messages`
- **THEN** the persisted requester message is returned and no run or daemon command is created

#### Scenario: Initial message starts explicitly

- **WHEN** the UI has persisted message M and calls clarification/start with M's ID and the current expected_state_version
- **THEN** the server creates/reuses the clarification run, returns its `run_id`, includes M in `session.start` context when assigned, and creates no `message.send` for M

#### Scenario: Later message dispatch is explicit

- **WHEN** the UI persists later message M and calls `/requirements/{requirement_id}/clarification/runs/A/messages/M/dispatch` with known run A
- **THEN** the server creates/reuses exactly one durable `message.send` mapping for M and does not create another conversation message or retarget the request to a newer run

#### Scenario: Cancellation-pending run rejects later dispatch

- **GIVEN** run A is `phase=active` with `cancel_requested=true`
- **WHEN** the UI attempts to dispatch later message M to A
- **THEN** dispatch fails/conflicts, creates no `message.send`, A remains active and competing, and repeated cancellation remains idempotent

#### Scenario: Persisted message survives cancellation/dispatch race

- **GIVEN** requester message M is persisted for assigned active run A
- **WHEN** cancellation commits `cancel_requested=true` before the UI dispatches M
- **THEN** M remains canonical conversation history, dispatch fails/conflicts without creating `message.send`, and the UI does not delete or roll back M

#### Scenario: Agent reply survives transport loss

- **WHEN** an agent message event was durably projected and the browser reconnects
- **THEN** the Conversation tab obtains that message from the HTTP conversation read, even if it observed no SSE event

#### Scenario: Unavailable dispatch does not erase a message

- **WHEN** requester message persistence succeeds but clarification start/dispatch cannot reach a daemon
- **THEN** the message remains visible in canonical conversation history and the UI shows operational unavailability separately from message loss

#### Scenario: Stale start preserves persisted history

- **WHEN** clarification/start returns HTTP 409 for the submitted expected_state_version
- **THEN** the UI keeps the already-persisted message, refetches canonical detail reads, and does not retry with a newer state version

#### Scenario: Reload can retry unassigned start deterministically

- **GIVEN** run A is an unassigned reusable clarification attempt
- **AND** the browser reloads and loses local mutation state
- **WHEN** the UI reads `GET /requirements/{requirement_id}/session`
- **THEN** the response includes A's `run_id`, `start_message_id`, `phase=awaiting_assignment`, and `status=unavailable`
- **AND** the UI can explicitly retry `/clarification/start` using that persisted start message
- **AND** no competing run is created

#### Scenario: Active phase rejects competing start after cancellation intent

- **WHEN** the session read contains run A with `phase=active`, including `status=unavailable` for a pinned disconnected daemon or `cancel_requested=true`, and the UI persists another message that is sent as a start request
- **THEN** the UI does not infer legality from `status`, shows the canonical active-run conflict, creates no second run, and refetches canonical detail reads

#### Scenario: Terminal phase starts a sequential run

- **WHEN** the session read contains run A with `phase=terminal` and the UI persists eligible message M2
- **THEN** identity-creating start uses M2 and the current expected_state_version, returns run B's `run_id` and `start_message_id`, and B becomes the rendered latest run while A remains server-owned history

#### Scenario: Stale run mutation cannot affect a newer run

- **GIVEN** browser state references run A
- **AND** run A becomes terminal
- **AND** run B is subsequently created
- **WHEN** the stale browser sends a dispatch or cancel targeting run A
- **THEN** the server evaluates only A according to its current eligibility, MUST NOT mutate or cancel B, and the UI does not substitute B's `run_id`

#### Scenario: UI does not select canonical context

- **WHEN** the UI starts clarification with a persisted message
- **THEN** it sends the message identity and expected state only, while North selects the deterministic bounded `session.start` excerpt and always retains `start_message_id`

#### Scenario: Assigned cancellation waits for terminal runtime fact

- **GIVEN** run A has `phase=active` and `cancel_requested=true`
- **WHEN** the UI receives a successful `session.cancel` `command_ack` but no terminal runtime event
- **THEN** the UI keeps A active and competing, does not offer a competing start, rejects later-message dispatch, permits repeated idempotent cancellation, and continues targeting cancellation at A's explicit `run_id`

#### Scenario: Unassigned cancellation releases slot immediately

- **GIVEN** run A has `phase=awaiting_assignment`, `status=unavailable`, and no `session.start`
- **WHEN** the UI cancels A
- **THEN** it renders A as `phase=terminal`, uses no daemon command, and permits a later eligible message to start run B

### Requirement: Overview renders structured state without transcript derivation

Overview SHALL render structured fields only from `GET /requirements/{id}` and
readiness/repository values only from the canonical readiness read. It SHALL
not summarize conversation text to derive title, description, summary,
criteria, assumptions, questions, status, revision, or state_version.

#### Scenario: Transcript absence cannot change the Overview

- **WHEN** conversation history is unavailable in a fault-injection test
- **THEN** the Overview's structured Requirement fields and canonical version/status values remain unchanged

### Requirement: Structured edits use expected state version

Every structured save SHALL send the current `expected_state_version` required
by the existing PATCH contract. The UI MAY display `revision` as content
version information, but SHALL never use it as the write precondition. On HTTP
409 Conflict, the UI SHALL refetch current canonical state and surface that the
Requirement changed; it SHALL not retry blindly with a new token.

#### Scenario: Stale edit is conflict-safe

- **WHEN** another actor changes a Requirement after the page loaded and the UI submits the old expected_state_version
- **THEN** the server returns 409, no local retry occurs, and the UI refetches and explains the conflict

### Requirement: Ready edits display server result

When a valid structured edit changes a Ready Requirement, the UI SHALL display
the status, revision, and state_version returned by the server, including the
domain-mandated Ready → Discussing demotion. It SHALL not predict the demotion
locally. No-op edits SHALL preserve the server's status/version behavior, and
terminal-state refusal SHALL be shown as an error.

#### Scenario: Ready edit demotes canonically

- **WHEN** a requester saves a real allowed field edit on a Ready Requirement
- **THEN** the UI renders the returned Discussing state and returned incremented version values

### Requirement: Activity is a canonical coarse read

The Activity tab SHALL refetch `GET /requirements/{id}/activity` and render only
server-persisted, intentionally product-visible coarse summaries. SSE SHALL
only hint that new activity may exist. The tab SHALL not require observation of
all notifications, transport frames, or raw runtime/tool diagnostics.

#### Scenario: Missed activity hint is harmless

- **WHEN** the browser misses activity notifications while disconnected
- **THEN** reconnect/refocus HTTP refetch returns the current retained activity read model

### Requirement: Session phase and status remain separate from retry state

The UI SHALL consume the public `GET /requirements/{id}/session` projection,
including `run_id`, `start_message_id`, `phase`, coarse `status`,
`cancel_requested`, and safe timestamps. `phase` SHALL be
`awaiting_assignment`, `active`, or `terminal` and SHALL determine legal
clarification actions; `status` SHALL remain display-only coarse operational
health/result (`starting`, `running`, `completed`, or `unavailable`). The UI
shall not infer action legality from `status=unavailable` alone. `phase=active`
continues to occupy the competing slot when a pinned daemon is disconnected or
cancellation is pending. `cancel_requested` is user intent and does not itself
make an assigned run terminal. Later-message dispatch is legal only when
`cancel_requested=false`; cancellation remains legal and idempotent while the
assigned run stays active. A newer sequential run replaces an older run as
the rendered latest projection; older runs remain server-owned history and need
not be exposed as a full run-history UI. The UI SHALL not require or define the
later retry state machine, attempt count, retry budget, server backoff, or
terminal execution failure semantics.

#### Scenario: Phase disambiguates unavailable status

- **WHEN** the session read reports `status=unavailable`
- **THEN** the UI uses `phase=awaiting_assignment` to offer only same-start retry/cancel, `phase=active` to keep the run competing without a new start and reject later
dispatch when cancellation is pending, and `phase=terminal` to allow a new
eligible start, while retaining canonical Requirement lifecycle/status

### Requirement: Reconnect and refocus refetch all canonical detail state

After SSE disconnect, EventSource reconnect, page reload, browser refocus, or a
relevant notification, the detail view SHALL refetch Requirement, conversation,
latest/current readiness, coarse activity, and the public session projection.
Missed, duplicate, delayed, and out-of-order hints SHALL not duplicate messages,
Requirement transitions, edits, or activity entries. The frontend SHALL never
open a WebSocket.

#### Scenario: Duplicate hint causes no duplicate projection

- **WHEN** the same notification is received twice
- **THEN** the UI may coalesce or repeat harmless HTTP reads, but rendered state comes from canonical responses and no item or mutation is duplicated

### Requirement: Detail telemetry is intentionally private

The detail surface SHALL never render chain-of-thought, raw tool output,
credentials, checkout paths, provider traces, or unfiltered runtime diagnostics.
Activity and failure displays SHALL use only coarse summaries intentionally
published by the server/adapter.

#### Scenario: Raw diagnostic is not rendered

- **WHEN** an upstream activity payload contains hidden reasoning or tool output
- **THEN** it is filtered or mapped before the UI and cannot appear verbatim in any detail tab

### Requirement: Detail scope stays requester-facing

The initial detail surface SHALL contain Conversation, Overview, and Activity
only, with repository citations and readiness visibility where canonical data
exists. It SHALL not add a Files tab, attachments, raw execution controls, or
new lifecycle mutation semantics.

#### Scenario: No file browser is implied

- **WHEN** an assessment cites a repository
- **THEN** Overview shows the canonical repository identity/full SHA evidence without exposing a source-file browser or checkout path
