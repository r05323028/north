# Migrations

Versioned SQL migrations for the relational database, applied by `north-server`
at startup (sqlx-style naming: `<version>_<description>.sql`).

The first migration lands with the OpenSpec change
`introduce-email-auth-and-owner-bootstrap` (users, roles, first-owner atomic claim).
Subsequent migrations accompany their owning changes (requirements, conversations,
readiness assessments, daemon registrations, runtime events + TTL, configured
repositories, and durable protocol delivery). The published `main` history ends
at 0012. New migrations must be strictly after that head and are ordered
`0013_repositories.sql` before `0014_protocol_delivery.sql` because protocol
backfill depends on the historical session/readiness tables plus repository
identity. Never insert a new migration into the 0001–0012 history.
