# Migrations

Versioned SQL migrations for the relational database, applied by `north-server`
at startup (sqlx-style naming: `<version>_<description>.sql`).

The first migration lands with the OpenSpec change
`introduce-email-auth-and-owner-bootstrap` (users, roles, first-owner atomic claim).
Subsequent migrations accompany their owning changes (requirements, conversations,
readiness assessments, daemon registrations, configured repositories, durable
protocol delivery, and clarification runtime). Runtime-event retention remains a
separate pending change; it does not describe `0015_clarification_runtime.sql`.
The checked-in `main` history ends at 0015. New migrations must be strictly after
that head. Migrations `0013_repositories.sql` and `0014_protocol_delivery.sql`
were ordered because protocol backfill depends on the historical
session/readiness tables plus repository identity; `0015_clarification_runtime.sql`
extends execution sessions and adds clarification activity storage. Never insert
a new migration into the 0001–0015 history.
