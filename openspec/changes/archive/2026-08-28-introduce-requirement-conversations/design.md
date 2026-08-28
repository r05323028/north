# Design

## Context

Conversation must stay context, not truth. The risk is agents or clients
deriving requirement state by replaying messages.

## Decisions

- Messages table: id, conversation_id, author_user_id (nullable for agent),
  kind (requester|agent|system), body, created_at. No message ever carries
  lifecycle authority.
- Structured edits are a separate conversation endpoint applying domain
`apply_edit`; the request includes `expected_revision` and the response returns
the new revision. Persistence performs the atomic match before invoking the
domain; stale edits return HTTP 409 with no message or requirement side effect.
- Agent messages arrive exclusively via protocol events (later change); this
  change defines their persistence shape only.

## Open Questions

None.
