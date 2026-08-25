# Design

## Decisions

- Ephemeral tables (runtime_events, activity) carry expires_at set at insert;
  retention_days config; periodic GC job deletes expired rows in batches.
- GC runs inside north-persistence behind a small interface invoked by server
  scheduler; never touches durable tables (enforced by test asserting table set).
- No external job runner; boring loop suffices at North scale.

## Open Questions

Default retention window (suggest 7 days) finalized during implementation.
