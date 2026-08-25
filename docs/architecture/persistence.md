# Persistence

Relational database, single source of truth for durable product data.
Migrations: versioned SQL in `../migrations/`, applied by the server at startup.

## Durable vs ephemeral

| Class | Examples | Rules |
| --- | --- | --- |
| Durable | users, roles, requirements (+revisions), readiness assessments, conversations, messages, configured repositories, human review decisions, daemon registrations/session summaries | never auto-deleted; deleting anything here is a product decision |
| Ephemeral (TTL) | runtime events, tool activity, transient execution logs | GC'd by a boring TTL job; expiry must never invalidate a Requirement |

Invariants:

- Ephemeral runtime data is never the sole source of truth for requirement state.
- TTL/GC deletes only ephemeral records; retention window is configuration.

## First-owner bootstrap

On a fresh instance, the first successfully created account becomes Owner via an
**atomic database operation** (e.g., singleton instance row claimed with a
unique constraint inside the signup transaction). Two simultaneous first
sign-ups must yield exactly one Owner; losers become normal Requesters.
No application-level "check then insert" races.

## Ownership mapping

- Domain types live in `north-domain`; row↔domain mapping lives in
  `north-persistence` (it may depend on domain for that mapping, on nothing else).
- Hosts never hand-roll SQL outside persistence.
