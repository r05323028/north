# Introduce Requirement conversation/detail UX

## Why

The requester's primary workflow lives here: converse, watch structured state
evolve, see honest execution health, without ever staring at raw model output.

## What Changes

- Detail route with Conversation / Overview / Activity tabs.
- Conversation pane: post messages, watch agent replies arrive via SSE; after
  disconnect/reconnect it refetches canonical conversation/Requirement state,
  not a durable stream replay.
- Overview pane: structured fields (summary, criteria, assumptions, open
  questions), lifecycle + execution badges, inline editing where allowed
  (revision bump visible).
- Activity pane: high-level agent activity entries only.
- Inspected repositories shown when assessments cite them. Ready badge when
  Ready. No Files tab, no raw thinking, no attachments.

## Capabilities

### New Capabilities

- `requirement-detail-ui`: tabbed detail surface binding conversation,
  structured overview, and activity honestly to underlying state.

### Modified Capabilities

- `conversations`: gains its primary UI consumer.

## Impact

- Affected docs: docs/product/conversation.md (UI consequences section).
- Dependencies on earlier changes: introduce-requirement-conversations,
  introduce-requirement-board.
