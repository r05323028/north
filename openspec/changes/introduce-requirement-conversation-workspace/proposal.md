# Introduce Requirement conversation workspace

## Why

North now has a canonical Requirement detail route, durable conversations, and
server-owned clarification runtime APIs, but `/requirements/[id]` remains a
read-only projection. Requesters need one direct-addressable workspace where
they can converse while watching the live structured Requirement change without
turning browser state, SSE, or runtime telemetry into a second source of truth.

This is next after the Board and clarification-runtime slices. It consolidates
those existing contracts into a requester-facing workflow before human review
and later retry-state UI are added.

## What Changes

- **[Invariant]** Extend the Board-owned `/requirements/[id]` route into a
  two-part workspace: primary Conversation and canonical Live Requirement. Keep
  deep links, refresh, browser navigation, and the existing Board foundations.
- **[Invariant]** Use the existing durable one-conversation-per-Requirement
  model. Render requester/agent/system messages in server order; represent
  clarification questions as ordinary `agent` messages and high-value progress
  as the existing coarse activity projection. Do not add a second message kind,
  transcript store, or runtime/transport identity to the browser model.
- **[Invariant]** Keep message persistence separate from runtime intent: persist
  requester input first, then use explicit clarification start or explicit
  `run_id`-scoped dispatch/cancellation operations. Preserve server arbitration,
  at-most-one non-terminal sequential run, durable command identity, and all
  existing cancellation/unavailability semantics.
- **[Invariant]** Render Live Requirement fields, readiness currentness, and
  version tokens from canonical HTTP reads. Inline structured edits, where
  allowed by the existing contract, send `expected_state_version` and never
  predict Ready demotion or overwrite a newer server response.
- **[Invariant]** Treat `requirement.changed`, `conversation.changed`,
  `readiness.changed`, `activity.changed`, and `session.changed` as invalidation
  hints only. Initial load, relevant hints, SSE reconnect, browser focus, and
  visibility repair refetch canonical HTTP state; duplicate, delayed, reordered,
  or missed hints remain harmless.
- **[Invariant]** Make composer states explicit: idle, submitting, active
  clarification, awaiting runtime assignment, cancellation pending, completed,
  and operational failure. Never silently drop a submitted message, retry a
  stale write, create a browser-side run, or start a second run during an
  occupied sequential slot.
- **[Invariant]** Obtain current-user identity and role from `/auth/me` for
  requester-facing labels and affordances. All authenticated roles retain
  workspace-wide view/conversation/edit capabilities from the existing policy;
  review transitions remain server-gated to Requirement Manager, Admin, and
  Owner. UI checks are cosmetic.
- **[Implementation suggestion]** Consolidate touched browser API calls behind a
  small shared client and runtime validators for Requirement, Conversation,
  Message, ClarificationRun, Readiness, Activity, and current-user responses;
  reuse existing modules and dependencies rather than introducing a new data
  library.
- **[Implementation suggestion]** Use existing shadcn/ui primitives and Tailwind
  responsive layout: two visible panes on desktop and stacked Conversation then
  Live Requirement sections on small screens, with accessible labels and
  actionable error/retry controls.
- **[Invariant]** Add focused unit, browser-boundary, and E2E coverage for
  persistence-before-intent, run identity/concurrency, cancellation, stale
  writes, canonical refetch, reconnect/focus repair, privacy, permissions, and
  responsive/accessibility-critical states.

No database migration or North protocol redesign is proposed. Existing
persistence and runtime contracts are consumed; any missing public read needed
for this workspace must be a backward-compatible projection, not a duplicated
lifecycle.

Out of scope: attachments or files, message edit/delete/reactions, hidden
reasoning, raw prompts/tool traces/daemon logs, browser WebSocket, a new
provider/runtime abstraction, a new retry budget/attempt state machine,
automatic daemon migration, per-Requirement ACLs, new Requirement lifecycle
transitions, or replacing the human-review surface.

The active `introduce-requirement-conversation-ui` change is an earlier,
planning-only detail-UI predecessor with overlapping scope. This change is its
workspace successor; implementation MUST select one contract rather than apply
both. Retiring or archiving that predecessor is separate change-management
work.

## Capabilities

### New Capabilities

- `requirement-conversation-workspace`: requester-facing Conversation + Live
  Requirement workspace, explicit clarification intents, canonical reads,
  responsive behavior, and browser repair/error semantics.

### Modified Capabilities

None. Existing `conversations`, `requirements`, `readiness`, `roles`, and
`browser-reconnect` requirements remain authoritative; this capability composes
them and defines the new workspace behavior without changing their domain or
wire contracts.

## Impact

- Frontend: `apps/web/app/requirements/[id]/page.tsx`, existing
  `requirement-detail`/shell components, shared API/event helpers, new focused
  workspace components and tests.
- Existing APIs consumed: Requirement detail/edit, paged conversation and
  requester-message routes, clarification start/dispatch/cancel, latest session,
  readiness, coarse activity, current user, and authenticated `/events`.
- Backend: only backward-compatible response/client support if existing public
  projections cannot express safe error/current-user data; no new command API or
  browser transport.
- Dependencies: completed `introduce-requirement-board`, completed
  `introduce-agent-requirement-clarification`, completed
  `introduce-local-repository-inspection`, and canonical Requirement,
  conversation, readiness, role, delivery, session-ownership, and reconnect
  contracts. `introduce-human-requirement-review` and
  `introduce-runtime-retry-and-failure-state` remain separate consumers/extensions,
  not prerequisites for this workspace.
- Canonical docs to update with landed behavior: `docs/product/requirement-lifecycle.md`,
  `docs/product/roles-and-permissions.md`, `docs/architecture/overview.md`,
  `docs/architecture/persistence.md`, `docs/development/testing.md`, and the
  affected rows in `docs/development/invariants.md`.
