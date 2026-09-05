# requirement-conversation-workspace Specification

## Purpose

Provides one requester-facing, reconnect-safe workspace for durable
conversation history and the server-owned structured Requirement without
creating a second lifecycle, execution state store, or browser transport.

## ADDED Requirements

### Requirement: Existing Requirement route becomes a two-part workspace

The authenticated `/requirements/[id]` route SHALL remain directly addressable
from Board, List, creation, browser navigation, and refresh. It SHALL render two
coherent parts from canonical server reads: a primary Conversation workspace and
a Live Requirement panel. On desktop-sized screens both parts SHALL be visible
at once, with Conversation receiving primary interaction space. On small screens
Conversation SHALL appear before Live Requirement in one stacked flow; the
Requirement SHALL remain reachable without leaving the clarification route.
The workspace SHALL not create a second detail route or a browser-side source of
truth.

#### Scenario: Deep link loads complete workspace

- **WHEN** an authenticated user opens `/requirements/R` directly or refreshes it
- **THEN** North loads the canonical detail bundle for R and renders Conversation and Live Requirement for R without requiring navigation from Board

#### Scenario: Small screen keeps structured state reachable

- **WHEN** the workspace is rendered below its desktop breakpoint
- **THEN** Conversation remains usable first, Live Requirement remains reachable in the same route, and the composer is not hidden behind an inaccessible desktop-only pane

#### Scenario: Clarification runtime is unavailable

- **WHEN** R has no eligible daemon or its pinned daemon is offline
- **THEN** the workspace still renders persisted conversation history and the latest canonical Requirement state, with availability shown as an operational condition rather than a missing page

### Requirement: Conversation uses the existing durable model

The workspace SHALL use the one durable Conversation associated with R. Its minimum North 0.1 projection is a stable `conversation_id`, `requirement_id`, and creation timestamp, plus durable Messages with stable `message_id` (wire `id`), `conversation_id`, optional author-user identity, one existing kind (`requester`, `agent`, or `system`), bounded user-facing body, and creation timestamp; the server may retain a source-event/idempotency key for deduplication. A message relates to a clarification Run only through the existing start/dispatch correlation, not through a new product lifecycle field. It SHALL display messages in the server's deterministic `(created_at, id)` order and deduplicate
by stable message ID when pages or refetches overlap. An agent clarification
question SHALL be displayed as an ordinary `agent` message; North 0.1 SHALL NOT
add a `clarification_question` message kind or infer canonical Requirement
fields from question text. High-value progress SHALL come from the canonical
coarse activity read and SHALL NOT be inserted into the transcript as a
synthetic message.

The workspace SHALL keep product identity distinct from execution and transport
identity: `requirement_id`, `conversation_id`, and `run_id` are application
identities; protocol `session_id` is the existing wire identity carried by a
run; runtime/provider IDs are correlation facts only. Components SHALL consume
shared API/domain contracts and SHALL NOT define competing `Message`, run,
readiness, or Requirement entities.

#### Scenario: Messages remain ordered and unique after repair

- **WHEN** the same message is returned by a paged response, a duplicate hint, or two overlapping canonical refetches
- **THEN** it appears once, at its server-defined position, with its server-provided author, kind, body, and timestamp

#### Scenario: Previously loaded contiguous range repairs without omission

- **GIVEN** the client successfully loaded contiguous history from offset 0 through exclusive `prior_loaded_end_offset=4` over pages at offsets 0 and 2, with stable IDs A, B, C, and D
- **WHEN** a new message shifts an offset page position before reconnect, focus repair, or a relevant SSE hint
- **THEN** repair discards cached slices, restarts at offset 0, follows server-returned `next_offset` until the rebuilt range reaches at least 4 and IDs A-D are re-observed (fetching beyond numeric offset 4 if needed), then reassembles A-D and the new message exactly once with no omission from trusting an old offset slice

#### Scenario: Expanded history repairs after reconnect or focus

- **GIVEN** the previously loaded range reached the conversation endpoint's end and new messages arrive while SSE is disconnected
- **WHEN** EventSource reconnect, browser focus, visibility return, or explicit repair runs
- **THEN** the workspace follows the refreshed contiguous page chain through its new `next_offset` end, includes the new messages, deduplicates overlap, and ignores any older repair that completes later

#### Scenario: Durable history survives client and daemon restart

- **WHEN** the browser reloads, frontend reconnects after an SSE outage, or the daemon reconnects after a transport outage
- **THEN** authenticated HTTP reads reconstruct the same persisted Conversation Messages and public Run history without local-only recovery or duplicate entries

#### Scenario: Agent question is safe conversation content

- **WHEN** an agent asks a clarification question through a valid `agent.message` projection
- **THEN** the question appears as an agent conversation message and the requester may answer through the composer without a new message kind or local Requirement mutation

#### Scenario: Activity is not transcript content

- **WHEN** the server exposes a repository-inspection or clarification-progress activity
- **THEN** the workspace may show its coarse summary in activity/status UI, but it does not render it as a requester or agent message and does not show raw tool records

#### Scenario: Runtime identity does not become product identity

- **WHEN** a run includes protocol `session_id` or a provider/runtime correlation ID
- **THEN** the workspace uses the public `run_id` projection for application actions and does not expose daemon/provider identity as conversation ownership

### Requirement: Canonical detail reads repair all workspace state

The workspace SHALL obtain structured Requirement state, Conversation pages,
latest/current readiness, coarse activity, latest public clarification run, and
current-user identity from authenticated HTTP APIs. The first bundle MAY be
parallel, but rendering SHALL use each response only for its own resource. On
initial load, a relevant invalidation, EventSource reconnect, browser focus,
visibility return, or explicit retry, it SHALL refetch the affected canonical
bundle and SHALL not treat an SSE payload as state.

The shared authenticated SSE categories `requirement.changed`,
`conversation.changed`, `readiness.changed`, `activity.changed`, and
`session.changed` SHALL be treated as hints containing at most resource
identity/category. A hint for another Requirement SHALL not change R. Missed,
duplicated, delayed, reordered, malformed, or overrun streams SHALL be harmless:
the workspace SHALL retain last-known data where possible and repair through
HTTP. A displayed connection state SHALL reflect actual EventSource state; it
shall not claim connected while disconnected. HTTP refetch remains the
correctness mechanism even while SSE is connected.

Because the existing conversation endpoint is offset-based and returns `next_offset`, define `prior_loaded_end_offset` as the exclusive numeric offset immediately after the last message in the largest contiguous history range the client successfully loaded from offset 0. It is not a page index or arbitrary cached-page marker. Before repair, the client SHALL retain that range's stable message IDs and whether its final page returned `next_offset: null`. A canonical repair SHALL discard cached page slices, restart at `offset=0`, and let each server-returned `next_offset` control the next bounded-page request. It SHALL continue until the rebuilt range reaches at least `prior_loaded_end_offset` and every stable ID from the prior range has been re-observed. If shifted page positions require it, repair SHALL follow `next_offset` beyond the old numeric end until those IDs appear or the server reports its end. If the prior range reached the server's end, repair SHALL follow newly returned `next_offset` pages through the current end so messages added after the prior load are not omitted. The client MUST NOT independently reissue stale historical offsets or trust their cached page slices. The final union SHALL deduplicate by stable message ID and sort by authoritative `(created_at, id)` order. Each repair generation owns its result, so an older repair cannot overwrite a newer completed repair. This is a client repair contract; it does not introduce cursor pagination or a new backend endpoint.

#### Scenario: Initial bundle uses canonical endpoints

- **WHEN** the workspace first mounts for R
- **THEN** it requests R, R's conversation, readiness, activity, public session projection, and current-user data over authenticated HTTP and renders no placeholder Requirement fields derived from transcript text

#### Scenario: Relevant hint causes canonical repair

- **WHEN** the workspace receives a valid named SSE hint for R
- **THEN** it refetches canonical HTTP state for the workspace, and any returned Requirement/message/activity/run changes replace stale rendering without applying the hint as a local mutation

#### Scenario: Unrelated hint is ignored

- **WHEN** the workspace receives a valid hint whose `requirement_id` is not R
- **THEN** it does not alter R's rendered state or create a request targeted at R solely because of that hint

#### Scenario: Missed and duplicate hints do not duplicate state

- **WHEN** the browser misses a hint while disconnected or receives the same hint more than once
- **THEN** reconnect/focus repair obtains current HTTP state, and no message, activity item, Requirement transition, or run is duplicated

#### Scenario: Older bundle response cannot overwrite newer state

- **WHEN** a slower request started before a newer workspace refetch completes afterward
- **THEN** the older response is ignored for rendering and cannot restore an older Requirement, conversation, readiness, activity, or run projection

#### Scenario: Refresh failure keeps usable stale data

- **WHEN** a non-initial canonical refetch fails
- **THEN** last-known Requirement and conversation data remain visible where available, a refresh error is shown separately, and the workspace does not silently present stale data as confirmed current

### Requirement: Browser API boundary preserves existing wire contracts

The workspace API layer SHALL use these existing authenticated operations without
creating aliases, a generic command endpoint, or a protocol redesign:

- `GET /requirements/{requirement_id}` for canonical Requirement state;
- `GET /requirements/{requirement_id}/conversation?offset=&limit=` for paged messages;
- `POST /requirements/{requirement_id}/conversation/messages` for requester-message persistence;
- the existing structured Requirement `PATCH` operation with
  `expected_state_version`;
- `GET /requirements/{requirement_id}/readiness` and
  `GET /requirements/{requirement_id}/activity?offset=&limit=`;
- `GET /requirements/{requirement_id}/session` for the public latest-run
  projection;
- explicit clarification `start`, run-scoped `dispatch`, and run-scoped
  `cancel` operations; and
- `GET /auth/me` and the shared authenticated `GET /events`.

All JSON received for these resources SHALL be runtime-validated at the API
boundary. Unknown status/kind/phase/role values, missing required identifiers,
invalid positive version numbers, invalid arrays, and malformed response shapes
SHALL fail closed with an actionable API error rather than being cast into a
local type. The existing session response wrapper may be named `session` on the
wire, but the shared browser contract SHALL expose it as one
`ClarificationRun | null` concept and SHALL not define a second `Session` entity.

#### Scenario: Malformed canonical response is rejected

- **WHEN** an API returns an unknown Requirement status, an unsafe version number, or a message with an unsupported kind
- **THEN** the API layer reports invalid server data and the workspace does not render that value as a valid canonical entity

#### Scenario: HTTP errors retain server classification

- **WHEN** an API returns an HTTP status and JSON error code
- **THEN** the workspace retains both status/code classification so it can distinguish message submission, clarification availability/conflict, cancellation, refresh, authorization, and realtime errors

#### Scenario: Existing response names remain compatible

- **WHEN** the server returns the current `session` wrapper and snake_case Requirement/message fields
- **THEN** the workspace consumes those fields through one shared parser without requiring a backend rename or component-local DTO copy

### Requirement: Requester message persistence precedes clarification intent

Submitting composer content SHALL first persist one requester message through the
canonical conversation-message operation. The returned stable `message_id` and
server-created timestamp/body SHALL be retained. A successful persistence call
SHALL have no runtime side effect by itself: it SHALL not create a run, choose a
daemon, create `session.start`, or create `message.send`. The workspace SHALL
not clear or discard composer content until persistence succeeds; a persistence
failure SHALL leave content available for correction or retry.

After persistence, the workspace SHALL use explicit intent based on the
canonical run projection and its known identity:

- with no run, or with a terminal run, it SHALL call clarification `start` for
  the persisted message and current `expected_state_version`;
- with an assigned active run and `cancel_requested=false`, it SHALL dispatch
  the persisted message to that explicit `run_id`;
- with an awaiting-assignment run, it SHALL not submit a different message for
  dispatch or start while that run occupies the sequential slot; and
- with an active cancellation-pending run, it SHALL not dispatch a new message
  while that run occupies the slot.

A start response SHALL retain its returned `run_id` and `start_message_id`.
The initial start message SHALL not also be dispatched as `message.send`.
Dispatch and cancellation SHALL never be sent without an explicit known
`run_id`; a latest-run read SHALL guide presentation only and SHALL not silently
choose a mutation target.

#### Scenario: Persisted message has no runtime side effect

- **WHEN** a requester posts valid body text to the conversation-message operation
- **THEN** the message is returned and visible from canonical history, while no clarification run or daemon command is created by that operation

#### Scenario: New conversation starts explicitly

- **GIVEN** R has no clarification run and its current `state_version` is V
- **WHEN** the workspace persists message M and calls start with M and V
- **THEN** North creates or reuses the server-owned run according to its slot contract, includes M in server-selected start context when assigned, and creates no separate `message.send` for M

#### Scenario: Later input targets known active run

- **GIVEN** the workspace knows run A and A is `phase=active` with `cancel_requested=false`
- **WHEN** the requester submits later message M
- **THEN** North persists M first and the workspace calls dispatch for exactly `/requirements/R/clarification/runs/A/messages/M/dispatch`, never a latest-run or daemon endpoint

#### Scenario: Concurrent active input is durable and ordered

- **GIVEN** run A is active and not cancellation-pending
- **WHEN** two browser contexts submit different requester messages concurrently
- **THEN** both successful messages remain canonical history, each dispatch is bound to A, and durable server command sequencing decides their delivery order without creating a second run or silently dropping either message

#### Scenario: Duplicate submission does not duplicate intent

- **WHEN** the same persisted message is submitted to the same explicit run more than once because of a retry or browser race
- **THEN** the server's existing message-to-command identity contract makes the repeated dispatch idempotent, and the workspace renders one message without claiming a second runtime submission

#### Scenario: Initial persistence failure retains draft

- **WHEN** requester-message persistence fails before a message ID is returned
- **THEN** the workspace shows message-submission failure, keeps the text available, and does not call start, dispatch, or cancel

### Requirement: Composer states expose slot and runtime truth

The composer SHALL expose distinct user-visible states for idle initial input,
message submission, clarification start/dispatch in progress, active
clarification, awaiting runtime assignment, cancellation pending, completed
run, temporarily blocked input, and failure. It MAY serialize one local submit
chain for accidental double-click prevention, but server/persistence arbitration
SHALL remain authoritative for concurrent browser contexts.

While `phase=active` and `cancel_requested=false`, the composer SHALL accept
later messages even when coarse `status=starting`, `status=running`, or
`status=unavailable`; an unavailable pinned daemon is a waiting condition, not
permission to migrate the run. While `phase=awaiting_assignment` or
`phase=active` with `cancel_requested=true`, the workspace SHALL disable new
clarification submissions, preserve any unsent draft, and offer only the legal
same-start retry/cancel controls. It SHALL not silently post an input that
cannot be dispatched. After `phase=terminal`, a new persisted message MAY start
a new run using the existing start operation; it SHALL not reactivate or target
the old run.

#### Scenario: Awaiting assignment is explicit

- **WHEN** the public run is `phase=awaiting_assignment` and `status=unavailable`
- **THEN** the workspace shows that runtime assignment is unavailable, keeps the run's `start_message_id`, offers retry of that same start or cancellation, disables new clarification submission, and does not create another run

#### Scenario: Pinned offline run retains slot

- **WHEN** the public run is `phase=active`, `status=unavailable`, and `cancel_requested=false`
- **THEN** the workspace shows that the pinned runtime is unavailable, keeps the sequential slot occupied, allows explicit later dispatch for that run, and starts no replacement run or frontend retry loop

#### Scenario: Cancellation pending blocks input

- **WHEN** the public run is `phase=active` with `cancel_requested=true`
- **THEN** the workspace shows cancellation pending, disables later-message dispatch, keeps the run target unchanged, and permits only idempotent cancellation or canonical refetch

#### Scenario: Terminal run permits a new start

- **WHEN** the public run becomes `phase=terminal` and the requester submits a new message
- **THEN** the workspace persists that message and uses start to obtain a new run identity, while the previous run and its messages remain history

#### Scenario: Draft is not silently lost during blocked input

- **WHEN** a requester has typed content and the run changes to awaiting assignment or cancellation pending before submission
- **THEN** the composer retains the unsent content and explains why submission is temporarily disabled

### Requirement: Start conflicts, unavailability, and races preserve history

The workspace SHALL preserve every requester message for which persistence
succeeded, regardless of later start or dispatch outcome. A start conflict for a
genuinely new logical start SHALL refetch canonical detail state and SHALL not
retry with a newer `expected_state_version`. A matching same-message
concurrent/retried start SHALL be presented as reuse of the canonical run, not
as a stale-new-start failure. A different-message start while a non-terminal
run occupies the slot SHALL be presented as an occupied-slot conflict; its
persisted message remains history and is not automatically dispatched or
rolled back.

A `503 clarification_unavailable` result SHALL display runtime unavailability
separately from message persistence and retain the returned awaiting run
identity. A retry after reload SHALL use that run's public
`start_message_id`. A dispatch/cancellation conflict, unknown run, or
Requirement/run mismatch SHALL not be retargeted to another latest run.
If cancellation wins after message persistence but before dispatch, the message
SHALL remain history and no message command SHALL be created.

#### Scenario: Same-message Draft race resolves one run

- **GIVEN** R is Draft at `state_version=1` and persisted message M is submitted to start twice concurrently
- **WHEN** server arbitration resolves the two start requests
- **THEN** both results identify one canonical run, Draft becomes Discussing at most once, and the losing response is not shown as a stale-new-start failure

#### Scenario: Different messages preserve loser history

- **GIVEN** R has no non-terminal run and persisted messages M1 and M2
- **WHEN** starts for M1 and M2 race
- **THEN** one request establishes the slot, the other reports occupied-slot conflict, both messages remain canonical, and no second run or automatic loser dispatch exists

#### Scenario: Reload retries same unavailable start

- **GIVEN** the browser reloads after R received `503 clarification_unavailable` for M
- **WHEN** the session read returns the unassigned run with `run_id=A` and `start_message_id=M`
- **THEN** the workspace can explicitly retry start with M, reuses A, and does not create a second non-terminal run

#### Scenario: Stale new start does not overwrite

- **GIVEN** no non-terminal run occupies R and another mutation advances its state version after the workspace read
- **WHEN** start for persisted M returns HTTP 409
- **THEN** M remains in history, the workspace refetches canonical state, and it does not resubmit start with a guessed newer token

#### Scenario: Cancellation wins dispatch race

- **GIVEN** M is persisted for active run A and cancellation commits before M's dispatch
- **WHEN** dispatch for A/M executes
- **THEN** dispatch conflicts without creating `message.send`, M remains visible, and the workspace explains that it was saved but not sent to the cancelled run

#### Scenario: Stale run cannot affect newer run

- **GIVEN** run A becomes terminal and later run B becomes latest
- **WHEN** a delayed browser request still targets A
- **THEN** North evaluates only A, does not mutate or cancel B, and the workspace does not substitute B's identity into the request

### Requirement: Clarification phase, status, completion, and cancellation remain separate

The workspace SHALL consume the public run projection fields
`run_id`, `requirement_id`, `start_message_id`, `phase`, `status`,
`cancel_requested`, and safe timestamps. `phase` SHALL decide sequential-slot
ownership and legal intent:

- `awaiting_assignment` is an unassigned reusable run and occupies the slot;
- `active` is an assigned non-terminal run and occupies the slot, including a
  pinned disconnected daemon or cancellation-pending run; and
- `terminal` releases the slot after unassigned cancellation or a durably
  projected terminal runtime fact.

`status` SHALL remain coarse display information (`starting`, `running`,
`completed`, or `unavailable`) and SHALL not alone decide an action. In
particular, `unavailable` in awaiting and active phases has different meaning.
The workspace SHALL not add or require the later server-owned retry state,
attempt count, retry budget, backoff, automatic `session.resume`, or final
execution-failure policy.

Completion SHALL not imply `Ready`; only the canonical Requirement and
readiness read decide those values. A runtime failure SHALL be shown as an
operational run failure and SHALL not mark the Requirement failed or mutate its
revision/state version. An active run remains active after a cancellation command
is acknowledged until `session.completed` or `session.failed` is projected.
Successful assigned cancellation is represented by existing completed status
with `cancel_requested=true`; terminal cancellation failure is represented by
existing unavailable status with cancellation intent, not by inventing a
`cancelled` status. Unassigned cancellation is immediately terminal with no
command.

#### Scenario: Completion without assessment is not Ready

- **WHEN** a run completes before an accepted readiness assessment exists
- **THEN** the workspace shows a completed run, leaves Requirement lifecycle/status and versions unchanged, and shows no synthetic current readiness

#### Scenario: Readiness changes independently

- **WHEN** a readiness assessment is stale, rejected, or targets another revision
- **THEN** the workspace shows the canonical current flag/outcome and does not infer readiness from run completion or an agent message

#### Scenario: Assigned cancellation waits for terminal fact

- **WHEN** cancellation receives `command_ack` but no terminal runtime event
- **THEN** the workspace keeps the run active and slot-occupying, disables later dispatch and new start, and does not label command acknowledgement as cancellation completion

#### Scenario: Successful cancellation is distinct from failure

- **WHEN** a requested run terminates successfully and the server projects existing `session.completed`
- **THEN** the workspace shows cancellation completed using `cancel_requested=true` and completed status, without mapping it to failure or adding a new protocol status

#### Scenario: Cancellation preserves committed partial state

- **WHEN** an assigned run is cancelled after it has persisted agent messages, activity, or a canonical Requirement/readiness update
- **THEN** those committed facts remain visible after terminal projection; cancellation stops further work but does not roll back history or mark the Requirement failed

#### Scenario: Runtime failure leaves Requirement lifecycle alone

- **WHEN** a run reaches terminal unavailable state without cancellation intent
- **THEN** the workspace shows run failure/unavailability, retains any partial canonical messages and Requirement data, and does not change Requirement lifecycle, revision, or state version

#### Scenario: Unassigned cancellation releases slot

- **WHEN** an awaiting-assignment run is cancelled before any start command exists
- **THEN** the workspace shows terminal cancellation intent, no daemon command is assumed, and a later eligible message may use start for a new run

### Requirement: Live Requirement stays canonical and concurrency-safe

The Live Requirement panel SHALL render title, description, summary, acceptance
criteria, assumptions, open questions, lifecycle status, `revision`,
`state_version`, creator, and timestamps from the Requirement response. Current
readiness and repository citations SHALL come only from the readiness response;
repository citations may show retained repository identity and full commit SHA
but SHALL not show checkout paths or credentials. Conversation text SHALL never
be summarized into structured fields.

The workspace SHALL expose the existing structured Requirement edit contract in
this slice because the canonical conversations requirement makes structured
edits through the conversation surface part of North 0.1. This adds no new
backend edit semantics. Every save SHALL send the displayed `state_version` as
`expected_state_version` and shall use the server response as the next canonical
Requirement. The panel SHALL not optimistically predict a Ready-to-Discussing
demotion. A 409 SHALL keep the unsaved draft available for user reconciliation,
refetch the canonical bundle, and explain that the Requirement changed; it SHALL
not blindly retry. Accepted and Rejected Requirements remain subject to the
existing terminal edit rules, and no new lifecycle transition is introduced by
this workspace.

#### Scenario: Transcript cannot alter structured panel

- **WHEN** conversation history is missing, delayed, or contains text that resembles a Requirement field
- **THEN** the Live Requirement panel still renders only the last canonical Requirement response and does not derive or rewrite structured values

#### Scenario: Ready edit uses server result

- **WHEN** an allowed real edit is saved against a Ready Requirement
- **THEN** the request carries `expected_state_version`, and the panel displays the returned Discussing status, revision, and state version rather than predicting them

#### Scenario: Stale edit is conflict-safe

- **WHEN** another actor changes R before the panel saves its displayed state version
- **THEN** the server returns 409, no local retry occurs, the canonical bundle is refetched, and unsaved user input is not silently discarded

#### Scenario: Terminal edit shows server refusal

- **WHEN** a user attempts a structured edit against an Accepted or Rejected Requirement
- **THEN** the workspace shows the canonical refusal and does not mutate the Requirement or conversation

#### Scenario: Repository evidence stays interpretable

- **WHEN** a readiness response cites a retained or disabled repository
- **THEN** the panel displays the canonical repository ID and exact full SHA when available, without implying active selection or exposing daemon checkout details

### Requirement: Workspace permissions follow instance roles and server authority

All authenticated users in North 0.1.0 SHALL have workspace-wide access to view
Requirements and conversations, append requester messages, cancel an accessible
clarification run, and begin discussion through the existing operation,
regardless of creator identity. The workspace SHALL not infer per-Requirement
ownership or ACLs.

For structured content edits, the workspace SHALL use the existing Requirement
mutation contract: Requester, Requirement Manager, Admin, and Owner are each
allowed to edit non-terminal structured fields in the current North 0.1.0
policy, subject to validation, `expected_state_version`, and server terminality
rules. This permission changes canonical content only. It does not grant any
role authority to calculate or accept readiness, make review decisions, perform
reviewer-only lifecycle transitions, or mutate other server-owned state merely
because the workspace displays edit controls.

Role permissions SHALL remain:

| Role | View/converse/cancel | Edit non-terminal structured content | Begin discussion | Review lifecycle |
| --- | --- | --- | --- | --- |
| Requester | allowed | existing Requirement mutation contract | allowed | forbidden |
| Requirement Manager | allowed | existing Requirement mutation contract | allowed | allowed |
| Admin | allowed | existing Requirement mutation contract | allowed | allowed |
| Owner | allowed | existing Requirement mutation contract | allowed | allowed |

Accept, Reject, Request Changes, and Reopen SHALL continue to use their existing
server-side reviewer guard. The workspace MAY hide or disable review affordances
from a Requester, but a forged HTTP request SHALL be rejected by the server.
Likewise, client-side role checks SHALL never replace authentication, run
binding, terminality, or state-version enforcement.

The workspace SHALL obtain the current user's ID, email, and exact role from
`GET /auth/me` wherever it labels the current actor or decides an affordance. It
SHALL not render a hard-coded person, email, avatar identity, or role as a
fallback.

#### Scenario: Requester collaborates across ownership

- **WHEN** an authenticated Requester opens, posts to, or edits structured content on another user's non-terminal Requirement
- **THEN** the server applies the existing workspace collaboration and Requirement mutation contracts subject to canonical validation, and the UI does not require creator matching

#### Scenario: Reviewer role receives review affordance

- **WHEN** the current user is Requirement Manager, Admin, or Owner and R is reviewable
- **THEN** the workspace may expose the existing reviewer route/action, while the server remains responsible for reviewer authorization, readiness-generation, lifecycle, and state-version checks

#### Scenario: Requester cannot review by forging UI

- **WHEN** a Requester directly calls an Accept, Reject, Request Changes, or Reopen endpoint
- **THEN** server authorization rejects the request before lifecycle or audit mutation, regardless of workspace visibility

#### Scenario: Current actor is canonical

- **WHEN** the current-user response identifies user U with role Q
- **THEN** requester-facing identity and role labels use U/Q, and no hard-coded identity is shown

#### Scenario: Requester edit does not grant reviewer authority

- **WHEN** a Requester uses the workspace structured editor and then attempts readiness calculation/acceptance, Accept, Reject, Request Changes, Reopen, or another restricted lifecycle operation
- **THEN** the existing server guards allow only the permitted content edit and reject the reviewer or restricted operation before server-owned state changes

#### Scenario: Terminality remains server-enforced

- **WHEN** any role attempts to edit an Accepted or Rejected Requirement or target an invalid run
- **THEN** the server rejects the operation and the workspace renders that authoritative error rather than bypassing it locally

### Requirement: Activity, errors, privacy, and accessibility are actionable

The workspace SHALL display only server-published coarse activity summaries and
safe readiness/session state. It SHALL never render chain-of-thought, raw
prompts, hidden model context, raw tool-call traces, credentials, checkout
paths, provider SDK records, daemon logs, or unfiltered failure diagnostics.
Message bodies SHALL be rendered as text, not trusted HTML. Event IDs, daemon
IDs, runtime IDs, and command payloads SHALL not be presented as user-facing
execution controls.

User-visible errors SHALL distinguish at least: message submission failure,
clarification start/dispatch conflict, runtime unavailable before assignment,
pinned runtime unavailable, run failure, cancellation pending/completion/failure,
canonical Requirement refresh failure, and temporary SSE reconnect. A stale
canonical Requirement SHALL remain visible during non-initial failure where
possible. Recovery controls SHALL be explicit and safe: retry the same
unassigned start, retry a failed HTTP read, reconcile an edit, or cancel the
known run; there SHALL be no blind state-version retry, frontend polling loop,
or automatic new-run fallback.

Composer controls, status announcements, tabs/sections, error messages, and
activity updates SHALL have accessible names, keyboard operation, visible focus,
and appropriate live/status semantics. Responsive layout SHALL not depend on
hover or color alone to communicate availability, cancellation, failure, or
readiness.

#### Scenario: Raw runtime detail is not exposed

- **WHEN** an upstream runtime produces tool output, hidden reasoning, a checkout path, or a provider-specific error
- **THEN** the workspace omits it or displays only an intentional coarse server summary

#### Scenario: Errors identify recoverable action

- **WHEN** message persistence fails, a start returns 503, an edit returns 409, or SSE reconnects
- **THEN** the workspace shows different actionable states for those conditions and never tells the user that an unsent or stale operation succeeded

#### Scenario: Stale data is honest during refresh outage

- **WHEN** a refetch fails after canonical data was already rendered
- **THEN** that data stays visible with a separate refresh warning, and no local update claims it is newer than the last successful server read

#### Scenario: Keyboard user can operate composer

- **WHEN** a keyboard-only user reaches the Conversation pane
- **THEN** the message field, submit/retry/cancel controls, status announcement, and any activity/Requirement navigation have accessible names and can be operated without pointer hover
