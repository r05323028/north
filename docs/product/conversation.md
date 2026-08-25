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

Related: docs/product/readiness.md, docs/architecture/server-daemon-protocol.md.
