# Design

## Context

See `proposal.md` for motivation and scope. Current repository contracts make
this a composition change, not a new conversation backend:

- `apps/web/app/requirements/[id]/page.tsx` owns the directly addressable route
  and currently mounts the read-only `RequirementDetail` client component.
- `apps/web/lib/requirements.ts` owns the shared Requirement wire type/parser
  and fetch helpers. `useRequirementCollection` already demonstrates request
  generation checks, stale-data retention, focus/visibility repair, and shared
  SSE subscription.
- `apps/web/lib/requirement-events.ts` consumes one authenticated `/events`
  stream. The server already emits the five relevant categories with only
  `category` and `requirement_id`.
- `north-server` already exposes Requirement detail/edit, paged conversation,
  requester-message persistence, clarification start/explicit dispatch/cancel,
  latest session, readiness, coarse activity, current user, and event routes.
- `north-persistence` already stores one Conversation per Requirement, stable
  messages ordered by `(created_at, id)`, source-event dedupe, sequential
  clarification runs, durable command mappings, and coarse activities.
- `GET /auth/me` returns canonical `id`, `email`, and exact role strings
  `Owner`, `Admin`, `RequirementManager`, or `Requester`.

The server calls the public latest-run response `session`, while persistence
calls the product concept `ClarificationRun` and exposes `run_id`. The design
keeps the existing wire wrapper for compatibility and gives browser code one
run concept. The browser remains HTTP/SSE-only; WebSocket, daemon, protocol,
retry-policy, and repository credentials stay outside this surface.

The `introduce-requirement-conversation-ui` change was an overlapping,
unimplemented predecessor. It is marked superseded in its artifacts. This change is
the selected canonical successor; its route, capability, and task set are the
only ones implementation agents may apply. The predecessor contract and tasks
MUST NOT be implemented or merged alongside this change.

## Goals / Non-Goals

**Goals:**

- Replace read-only detail rendering with one durable Conversation + Live
  Requirement workspace while preserving route ownership and Board navigation.
- Keep the existing inline structured Requirement edit interaction in this
  vertical slice because canonical `conversations` requires edits through the
  conversation surface in North 0.1; consume its PATCH/state-version contract
  without adding reviewer, readiness, or lifecycle semantics.
- Give every requester message one explicit server-owned outcome: persisted
  history, start of a known run, dispatch to a known active run, or a visible
  blocked/unavailable state.
- Use shared validated API contracts, canonical bundle refetch, stable message
  identity, and explicit run identity for correctness under races and reconnects.
- Make current-user identity, role affordances, responsive layout, error states,
  and accessibility behavior testable.

**Non-Goals:**

- New database tables, message kinds, protocol frames, command APIs, runtime
  adapters, daemon migration, retry budgets, or execution attempt policy.
- A full historical run browser, per-message delivery ledger, attachments/files,
  message editing/deletion, raw telemetry, chain-of-thought, or human-review
  workflow redesign.

## Decisions

### 1. Extend route and render two stable regions

Keep `/requirements/[id]` and replace only its client detail body with a
workspace component. Use existing card, button, textarea, status, and tab
primitives. Conversation is the primary region; Live Requirement is a separate
landmark rather than a transcript-derived summary.

Use a two-column grid at the existing desktop breakpoint or wider, with the
Conversation column expanding and the Live Requirement column remaining readable
and visible. Use one document flow below that breakpoint: Conversation first,
then Live Requirement. A compact in-page navigation/heading may jump to the
Requirement region, but no mobile-only drawer is required. This is simpler than
maintaining two copies of panel state and keeps canonical Requirement accessible
while typing.

Remove the shell's hard-coded user card and hard-coded `已連線` claim. The
workspace may show current-user and SSE state only after reading their actual
canonical sources. A neutral shell label is preferable to a false identity or
connection assertion.

### 2. Normalize vocabulary at one API boundary

Use this mapping and do not introduce competing entities:

| Concept | Existing wire/source | Browser contract | Rule |
| --- | --- | --- | --- |
| Requirement | `RequirementResponse` | existing `Requirement` | Structured truth; `revision` is content identity and `state_version` is write concurrency. |
| Conversation | `ConversationResponse` | `ConversationPage`/`Conversation` | One thread per Requirement; paged durable context. |
| Message | `MessageResponse` | shared `Message` | `requester`, `agent`, `system`; stable `id`; server order. |
| Clarification run | `{ session: ... }` and `ClarificationRunResponse` | `ClarificationRun` | `run_id` is product identity; `phase` controls slot/action legality. |
| Run status | `status` | `ClarificationStatus` | `starting`, `running`, `completed`, `unavailable`; display only. |
| Readiness | `ReadinessResponse` | `ReadinessView` | `current` is server-computed; never infer from run/status. |
| Activity | `ActivityResponse` | `ActivityItem` | Coarse server-published progress, never transcript content. |
| Current user | `UserResponse` | `CurrentUser` | Canonical identity/role for labels and affordances. |

The `session` JSON key and protocol `session_id` remain compatibility terms at
the transport boundary. Browser actions use explicit `run_id`; runtime IDs,
command IDs, and daemon IDs are not alternate ownership fields. Agent questions
use `kind=agent`; no `clarification_question` kind or message metadata is added.

### 3. Keep API modules small and parsers strict

Retain existing Requirement exports so Board/List do not break. Move or add only
the shared request/error/parser seam needed by this route:

```text
apps/web/lib/api/client.ts          request + ApiError + JSON boundary
apps/web/lib/api/requirements.ts    Requirement reads/edits (re-export existing helpers)
apps/web/lib/api/conversations.ts  page read + requester post
apps/web/lib/api/clarification.ts  run/readiness/activity/start/dispatch/cancel
apps/web/lib/api/current-user.ts   /auth/me
```

Exact filenames may stay under existing `lib/` modules if the same single-owner
contracts result. Do not add a state-management or schema dependency for these
small shapes. Hand-written runtime validators follow the existing
`parseRequirement` pattern and reject unknown closed enums, missing IDs,
non-positive unsafe version numbers, malformed arrays, and invalid wrappers.
`ApiError` retains HTTP status and server `error` code so UI state does not
parse human text to decide whether a conflict, unavailable response, or
authorization failure occurred.

Use existing paths and field names. The client calls `PATCH
/requirements/{id}` as the canonical structured edit path; the existing
`/conversation/structured` alias remains untouched. It parses the current
`{session: ...}` response into the one `ClarificationRun` contract but does not
rename the server response or add a second session model.

### 4. Load a repairable detail bundle

Implement one workspace load state containing the Requirement, loaded
conversation pages, readiness, activity, latest run, current user, loading/
refreshing flags, refresh error, and SSE connection state. Initial reads can use
`Promise.all`; each response is assigned only to its own resource.

Use a monotonically increasing bundle request generation. Every refetch captures
the generation and applies results only if still current. Keep the last
successful bundle during non-initial failures and mark it stale with a separate
refresh error. A successful mutation response may update its own resource, but a
canonical bundle refetch follows intent mutations and all relevant SSE hints.

Fetch the existing bounded conversation page shape. If `next_offset` exists,
provide an explicit load-more action. Merge pages by message ID and render the
server's `(created_at, id)` order. Because this API is offset-based, a repair
that has loaded pages through boundary B SHALL discard cached page slices, start
at `offset=0`, and follow the contiguous `next_offset` chain using the bounded
limit. It SHALL continue at least through B and until every stable message ID
from the cached pages has been re-observed; if a new message shifted a boundary,
follow additional returned pages until those IDs appear or the server reports
its end. If the loaded range had reached the prior end, follow newly returned
`next_offset` pages through the current end so expanded history is not omitted.
Deduplicate the refreshed union by ID, sort by `(created_at, id)`, and never
synthesize a gap-filling message from an SSE payload. A repair generation owns
its result, so an older page chain cannot replace a newer repair. Activity uses
the same bounded-page approach and displays only activity text/timestamps.

A relevant event is one of the five named categories whose
`requirement_id === id`. Coalesce synchronous bursts into one bundle refetch.
Reconnect, focus, and visibility return use the same refetch path; they do not
start polling or require `Last-Event-ID`. The shared subscription reports actual
`connecting`, `connected`, `reconnecting`, and closed/error state to the
workspace. EventSource is informational; HTTP remains correctness authority.

### 5. Use explicit composer state and server sequencing

The composer is a small state machine, not a client run manager:

| Canonical state | Composer behavior |
| --- | --- |
| No run or terminal run | Enable initial input. Persist message, then call `start` with current `state_version`. |
| Start/post/dispatch request pending | Disable duplicate submit, retain pending/error context, and wait for canonical response. |
| Awaiting assignment | Disable new clarification submissions; offer same `start_message_id` retry and explicit cancel. Preserve typed draft. |
| Active, no cancellation | Enable later input. Persist each message, then dispatch to the known run ID; `status=unavailable` means pinned waiting, not migration. |
| Active, cancellation requested | Disable later dispatch and new start; show cancellation pending and allow idempotent cancel/refetch. |
| Terminal completed/unavailable | Show outcome; allow a later eligible message to use a new explicit start. |

Persist first, retain returned `message_id`, and never roll back the message if
start/dispatch/cancel later fails. The initial message is in `session.start`
context and never gets `message.send`. Later messages always use the explicit
run ID known from the start response or canonical session read. A local
in-flight guard may stop double-clicks, but two tabs remain a server concern:
server command sequence allocation and the existing message-to-command mapping
provide durable order/idempotency.

An awaiting run is intentionally a submission boundary: the UI does not append
new input that it cannot legally dispatch. This avoids creating unexplained
orphan messages while no daemon is assigned; the user's unsent text remains in
the composer. If a state change races a successful message POST, the message
stays visible and the follow-up conflict explains whether it was not sent.

No browser operation infers intent from transcript text or from `status` alone.
No start retries a 409 with a newer version. A 503 awaiting response preserves
its returned run/start-message identity and offers an explicit same-message
retry. Dispatch/cancel paths never replace a stale run ID with latest-run data.

### 6. Map run and error states without inventing status values

Use `phase` first and `status` second:

| Read result | User-facing meaning | Allowed recovery |
| --- | --- | --- |
| `awaiting_assignment` + `unavailable` | Waiting for eligible runtime | Retry same start or cancel; no second start. |
| `active` + `starting`/`running` | Clarification active | Send later message or cancel using run ID. |
| `active` + `unavailable` | Pinned runtime temporarily unavailable | Keep slot; retain durable command; no migration or frontend retry loop. |
| `active` + `cancel_requested` | Cancellation pending | No later dispatch/new run; repeat cancel safely. |
| `terminal` + `completed`, no cancel | Run completed | Show completed; readiness still read independently. |
| `terminal` + `completed`, cancel | Cancellation completed | Show cancellation completion, not failure or a new `cancelled` status. |
| `terminal` + `unavailable`, no cancel | Run failed/unavailable | Show coarse operational failure; Requirement remains unchanged. |
| `terminal` + `unavailable`, cancel | Cancellation failed/unavailable | Show cancellation outcome separately from ordinary run failure. |

The existing public projection intentionally has no detailed failure reason and
later retry work owns richer execution state. Use bounded generic copy rather
than exposing stored daemon/provider diagnostics.

Map message POST errors separately from start/dispatch conflicts, 503 runtime
availability, cancel errors, canonical bundle refresh errors, authorization
errors, and SSE reconnect. An HTTP `202` means intent was accepted/persisted,
not that runtime work or cancellation completed. A successful canonical runtime
projection and subsequent refetch determine completion.

### 7. Preserve authorization and privacy at both boundaries

Read `/auth/me` in the workspace and compare its ID to requester message
`author_user_id` only when showing a current-user label such as “You”; unknown
other requesters remain “Requester” rather than being guessed from IDs. Use the
role only for cosmetic affordances. The server remains authoritative for
workspace-wide access, message append, cancellation/run binding, the existing
non-terminal structured-content edit contract, begin discussion, terminality,
readiness calculation/acceptance, and reviewer-only Accept, Reject, Request
Changes, and Reopen. Structured-content edit permission does not authorize a
Requester to perform those reviewer, readiness, or restricted-lifecycle
operations.

Render message bodies as text. Render only server-published coarse activity,
readiness verdict/currentness/blockers/assumptions, retained repository ID/full
SHA, safe timestamps, and structured Requirement fields. Do not display
provider traces, hidden reasoning, raw prompts/context, tool output, checkout
paths, credentials, daemon IDs, command envelopes, or runtime IDs. Do not put
any of those values into a client action URL.

### 8. Keep deployment and rollback boring

No migration runs for this change. Existing conversations, messages, runs,
activities, readiness evidence, and command records remain readable by old and
new clients. Deploy the compatible client against current endpoints; if a
backward-compatible parser/read projection is genuinely missing, add only that
projection and keep old response fields. Rollback is frontend-only: restore the
read-only detail renderer; it does not delete messages or alter run state.

After implementation, update the named canonical product/architecture/testing
pages from the proposal and change the invariant ledger only to statuses proven
by runnable tests. The predecessor is already marked superseded and is not an
implementation input; no second detail UI may be claimed or merged from it.

## Risks / Trade-offs

- **[Risk] Offset pagination shifts while new messages arrive.** → Discard
  cached slices on repair, restart at offset 0, follow contiguous `next_offset`
  pages far enough to re-observe every previously loaded ID (and through the new
  end when the prior range reached end), then deduplicate and sort by the server's
  `(created_at, id)`; never append SSE data or trust stale page offsets.
- **[Risk] A refresh error makes old Requirement data look current.** → Keep it
  visible for continuity, label it stale/refresh-failed, and clear the warning
  only after a successful canonical read.
- **[Risk] Public `unavailable` covers both no daemon and runtime failure.** →
  Use `phase`, `cancel_requested`, HTTP 503 context, and coarse activity for
  bounded user-facing distinctions; defer detailed failure/retry fields to the
  existing retry change.
- **[Risk] Current shell hard-codes identity and connectivity.** → Remove those
  claims and source any workspace labels/status from `/auth/me` and actual
  EventSource state.
- **[Risk] The prior conversation-UI change overlaps this route.** → Treat this
  change as successor and do not apply both task sets; no shared component may
  have two canonical implementations.
- **[Risk] A new client abstraction drifts from existing Board helpers.** → Keep
  existing Requirement exports stable, share one request/parser seam, and run
  Board/List tests with workspace tests.
