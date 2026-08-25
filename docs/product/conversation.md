# Conversation semantics

Conversation history is **supporting context**, not the canonical requirement.

Invariant:

> The structured Requirement state is the source of truth. Consumers must never
> need to replay conversation messages to know the current specification.

Consequences:

- The requirement detail view renders structured fields from requirement state,
  never by summarizing messages on the fly as truth.
- Messages explain how the requirement evolved; they do not redefine it.
- Raw model chain-of-thought is never exposed.
- Raw tool/runtime logs are activity telemetry, not conversation content and not
  specification content (see docs/architecture/persistence.md for retention).

## Live updates and edits

Structured edits carry `expected_revision`; stale saves return HTTP `409
Conflict` and do not append a message or mutate Requirement state. Browser SSE
is a notification hint only. After disconnect/reconnect, the detail view
refetches canonical Requirement and conversation state over HTTP; it never
reconstructs specification truth by replaying the stream.

Related: docs/product/readiness.md, docs/architecture/server-daemon-protocol.md.
