# Design

## Data flow and canonical reads

The detail page owns no daemon connection. It uses the authenticated server
API:

```text
GET /requirements/{id}
GET /requirements/{id}/conversation?offset=0&limit=...
GET /requirements/{id}/readiness
GET /requirements/{id}/activity?offset=0&limit=...
GET /requirements/{id}/session
```

The first four/five reads may be fetched in parallel. The Requirement response
is the sole source for structured fields, lifecycle status, `revision`, and
`state_version`. The conversation response is the sole source for messages.
The readiness response supplies the latest immutable assessment and its
`current` status plus repository IDs/full SHAs. Activity and session responses
are server projections, not transport caches. The existing Ready-only
`review-packet` remains a separate reviewer projection.

## Tabs

- **Conversation**: render persisted requester/agent/system messages in the
  existing deterministic order. Post through the canonical message endpoint;
  render the returned persisted message immediately. A start-of-run message
  follows clarification orchestration's durable-first/start-context rule; a
  later message uses `message.send` only after its message row exists.
- **Overview**: render title, description, summary, acceptance criteria,
  assumptions, open questions, lifecycle status, content revision, current
  readiness, and cited repositories from HTTP responses. Never derive fields by
  summarizing transcript messages.
- **Activity**: refetch `/activity` and render safe coarse summaries only. An
  SSE notification is a hint to refetch; it is not an activity entry.

## Optimistic concurrency and Ready edits

All structured saves include `expected_state_version` in the PATCH body used by
the current structured-edit API (including the conversation structured route).
`revision` is displayed as content-version information and is never used as the
write precondition. On HTTP 409, discard the stale local save, refetch the
Requirement/conversation/readiness/activity/session bundle, and show that the
Requirement changed. Do not auto-retry with the new token.

When a real edit changes a Ready Requirement, the server/domain operation
returns the canonical Discussing status and incremented revision/state_version.
The UI renders those response values. It does not locally predict or patch the
Ready demotion. No-op edits preserve the server's no-op behavior; Accepted and
Rejected edits surface the server error.

## Reconnect and notification behavior

The shared authenticated SSE endpoint emits lightweight categories such as
`requirement.changed`, `conversation.changed`, `readiness.changed`,
`activity.changed`, and `session.changed`. On any relevant hint, disconnect,
EventSource reconnect, browser refocus, or page reload, refetch:

1. Requirement;
2. conversation page(s);
3. latest/current readiness assessment;
4. coarse activity; and
5. minimal session/runtime status.

The page can coalesce refetches. It must not use `Last-Event-ID` or stream
history as a correctness dependency and must never open a WebSocket.

## Privacy and scope guards

Render only persisted message bodies, structured fields, repository IDs/full
SHAs, safe readiness values, coarse activity summaries, and minimal status.
Provider traces, raw tool output, hidden reasoning, credentials, checkout
paths, and internal diagnostics are dropped or mapped to generic summaries
before they reach the UI. Later retry/failure badges can extend the status
read model; they do not change these boundaries.
