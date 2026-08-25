# Design

## Context

Wire format must survive reconnects, retries, and future evolution without
leaking business types across crates.

## Decisions

- Envelope: { id (uuid), session_id (uuid), type, payload, sent_at,
  schema_version }. Commands vs events are distinct enum families in
  north-protocol; serde tagged representation (`type`).
- Idempotency: senders retry with same id; receivers keep recent-id window
  (per session) to dedupe; durable effects keyed by event/command id where
  they mutate state.
- Resume: daemon buffers unacked events on disk; on reconnect sends
  session.resume; server acknowledges with last-processed ids; gaps replayed.
- north-protocol stays serde-only; domain conversions happen in hosts.

## Open Questions

- Buffer format (JSONL append-log suggested) decided at implementation.
