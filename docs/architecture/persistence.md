# Persistence

Relational database, single source of truth for durable product and server
reliability state. Migrations: versioned SQL in `../migrations/`, applied by the
server at startup.

## Durable vs ephemeral

| Class | Examples | Rules |
| --- | --- | --- |
| Durable business | users, roles, requirements (+revisions), readiness assessments, conversations, messages, configured repositories, human review decisions | never TTL-deleted; deletion is a product decision |
| Durable coordination | daemon registrations, `session.daemon_id`, session execution state/attempts, server command outbox, event dedupe/rejection records, sequence watermarks | transactionally maintained; command payloads may be compacted only at the protocol's acknowledged sequence boundary |
| Ephemeral (TTL) | runtime events, tool activity, transient execution logs | GC'd by a boring TTL job; expiry must never invalidate a Requirement |

The daemon also keeps a local transport journal for command inbox and event
replay. That journal is not server business state, not `north-persistence`, and
not a second database authority. Its processed-command high-water tombstone is
retained for the durable session and is not expired by time alone in 0.1.0.

Invariants:

- Ephemeral runtime data is never the sole source of truth for requirement or
  execution policy state.
- TTL/GC deletes only ephemeral records; retention window is configuration.
- Server command outbox rows are inserted before dispatch and remain eligible
  for resend until `command_ack` is durably recorded.
- Every mutation of an existing Requirement uses an atomic `expected_revision`
  check; stale callers receive a conflict with no side effects.
- `requirement.assessed` dedupe, revision validation, domain gates, immutable
  evidence, lifecycle transition, and resulting row update share one
  transaction. Event ACK follows commit; invalid/stale facts commit a durable
  rejection record before `event_ack(status=rejected)`.

## First-owner bootstrap

On a fresh instance, the first successfully created account becomes Owner via an
**atomic database operation** (e.g., singleton instance row claimed with a
unique constraint inside the signup transaction). Two simultaneous first
sign-ups must yield exactly one Owner; losers become normal Requesters.
No application-level "check then insert" races.

## Repository identity

Configured repositories contain metadata only: id, name, URL, description,
timestamps, and nullable `disabled_at`. Normal Remove sets `disabled_at`; it
does not hard-delete a row referenced by assessment evidence. New inspections
exclude disabled rows, while historical id + metadata + exact commit SHA remain
human-readable.

## Ownership mapping

- Domain types live in `north-domain`; row↔domain mapping lives in
  `north-persistence` (it may depend on domain for that mapping, on nothing else).
- Hosts never hand-roll SQL outside persistence.
- Repository credentials never enter server persistence; daemon host Git
  configuration remains the credential boundary.
