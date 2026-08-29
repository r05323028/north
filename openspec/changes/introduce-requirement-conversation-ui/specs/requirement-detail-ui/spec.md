## Purpose

Renders a Requirement detail surface from canonical server reads: persisted
dialogue, structured Requirement state, current readiness evidence, coarse
activity, and minimal clarification status.

## ADDED Requirements

### Requirement: Detail uses canonical HTTP read models

The detail route SHALL read Requirement, conversation, latest/current readiness,
coarse activity, and minimal session/runtime status from authenticated HTTP
APIs. It SHALL not read daemon WebSocket traffic, interpret protocol events as
product truth, or reconstruct any model from an SSE replay log. The page SHALL
offer Conversation, Overview, and Activity tabs.

#### Scenario: Refetch restores the complete detail bundle

- **WHEN** the detail page loads or regains focus
- **THEN** it refetches the Requirement, conversation, readiness, activity, and session reads and renders each from its corresponding canonical response

### Requirement: Conversation is persisted and durable-first

Conversation SHALL render requester and agent messages returned by the canonical
conversation API. Posting a requester message SHALL follow clarification's
ordering: persist the message first, then enqueue/replay its durable runtime
command using the persisted message identity. The initial message that starts a
run SHALL be represented in `session.start` context and SHALL not be submitted
again as `message.send`. Duplicate/replayed delivery of that persisted message
SHALL not add another logical message or runtime submission.

#### Scenario: Agent reply survives transport loss

- **WHEN** an agent message event was durably projected and the browser reconnects
- **THEN** the Conversation tab obtains that message from the HTTP conversation read, even if it observed no SSE event

#### Scenario: Unavailable dispatch does not erase a message

- **WHEN** requester message persistence succeeds but no eligible daemon can receive its command
- **THEN** the message remains visible in canonical conversation history and the UI shows operational unavailability separately from message loss

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

### Requirement: Minimal session status does not require retry state

The UI MAY show only the clarification change's coarse session status
(`starting`, `running`, `completed`, or `unavailable`) and cancellation intent
from `GET /requirements/{id}/session`. It SHALL not require or define the later
retry state machine, attempt count, retry budget, server backoff, or terminal
execution failure semantics. `unavailable` SHALL remain operational status and
SHALL not be displayed as a Requirement lifecycle failure.

#### Scenario: Runtime unavailability leaves Requirement truth alone

- **WHEN** the session read reports unavailable
- **THEN** the UI shows operational unavailability while retaining the canonical Requirement lifecycle/status response

### Requirement: Reconnect and refocus refetch all canonical detail state

After SSE disconnect, EventSource reconnect, page reload, browser refocus, or a
relevant notification, the detail view SHALL refetch Requirement, conversation,
latest/current readiness, coarse activity, and minimal session/runtime status.
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
