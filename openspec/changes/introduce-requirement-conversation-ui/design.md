# Design

## Decisions

- Route /requirements/[id] with three tabs (conversation default); SSE carries
  notification hints for new messages/activity/status badges. EventSource
  reconnect/refocus refetches canonical HTTP state; no UI truth is reconstructed
  from SSE replay.
- Overview renders structured fields verbatim from the API — no client-side
  derivation of truth.
- Edit affordances inline on Overview fields (requester-editable states only);
  each save calls the structured-edit endpoint and surfaces revision change.
- Activity list maps agent.activity events to human-readable lines.

## Open Questions

None.
