# Introduce conversations and structured requirement editing

## Why

Clarification happens through dialogue while the structured requirement — not
the transcript — remains the deliverable. This change wires conversations to
requirements and makes revision-bumping edits a first-class flow.

## What Changes

- One conversation per requirement; messages from requester and agent
  (agent delivery arrives with the runtime changes — here we persist and
  render what the protocol will carry).
- Message kinds requester/agent/system; raw tool output never becomes a
  message.
- Structured requirement editing through the domain's apply_edit contract:
  revision bumps only on real content change, Ready demotion honored end-to-end.
- Conversation APIs paginate; structured state is always readable directly,
  independent of messages.

Out of scope: attachments/file uploads, message editing/deleting, reactions,
notifications.

## Capabilities

### New Capabilities

- `conversations`: per-requirement threads, message kinds, pagination,
  context-not-truth guarantees, structured-edit flow.

### Modified Capabilities

- `requirements`: content edits now arrive through the conversation surface
  as well as direct API (same domain rules).

## Impact

- Affected docs: docs/product/conversation.md (canonical),
  docs/product/requirement-lifecycle.md (edit demotion reference).
- Dependencies on earlier changes: introduce-requirement-domain-model.
