# Design

## Context

Conversation must stay context, not truth. The risk is agents or clients
deriving requirement state by replaying messages.

## Decisions

- Messages table: id, conversation_id, author_user_id (nullable for agent),
  kind (requester|agent|system), body, created_at. No message ever carries
  lifecycle authority.
- Structured edits are a separate endpoint applying domain `apply_edit`; the
  response returns the new revision so UIs can show staleness.
- Agent messages arrive exclusively via protocol events (later change); this
  change defines their persistence shape only.

## Open Questions

None.
