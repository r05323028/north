# Migrations

Versioned SQL migrations for the relational database, applied by `north-server`
at startup (sqlx-style naming: `<version>_<description>.sql`).

The first migration lands with the OpenSpec change
`introduce-email-auth-and-owner-bootstrap` (users, roles, first-owner atomic claim).
Subsequent migrations accompany their owning changes (requirements, conversations,
readiness assessments, repositories, daemon registrations, runtime events + TTL,
and durable protocol delivery watermarks/event identity records).
