# Introduce server-daemon protocol

## Why

Server and daemon need one boring, explicit contract: typed commands/events
with stable ids, at-least-once delivery, and idempotent processing, so
reconnects are unremarkable and duplicates are harmless.

## What Changes

- `north-protocol` fills with the envelope contract and baseline messages:
  commands session.start / session.cancel / session.resume / message.send;
  events session.started / agent.message / agent.activity /
  requirement.assessed / session.completed / session.failed.
- Stable command_id/event_id/session_id everywhere; dedupe windows server-
  side; daemon-side buffering of unacknowledged events across reconnects.
- Version field for forward compatibility.

Repository preparation events stay out unless later changes prove genuine
protocol value.

## Capabilities

### New Capabilities

- `daemon-protocol`: envelope shape, message catalog, delivery/idempotency
  guarantees, resume semantics.

### Modified Capabilities

(none)

## Impact

- north-protocol gains serde/serde_json/uuid deps (allowed; still no business
  crates).
- Affected docs: docs/architecture/server-daemon-protocol.md (canonical).
- Dependencies on earlier changes: introduce-daemon-runtime-connection.
