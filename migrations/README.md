# Migrations

Versioned SQL migrations for the relational database, applied by `north-server`
at startup (sqlx-style naming: `<version>_<description>.sql`).

The first migration lands with the OpenSpec change
`introduce-email-auth-and-owner-bootstrap` (users, roles, first-owner atomic claim).
Subsequent migrations accompany their owning changes (requirements, conversations,
readiness assessments, daemon registrations, configured repositories, durable
protocol delivery, and clarification runtime). Runtime-event retention remains a
separate pending change; it does not describe `0015_clarification_runtime.sql`.
The checked-in migration history currently ends at 0019. New migrations must be strictly after
that head. Migrations `0013_repositories.sql` and `0014_protocol_delivery.sql`
were ordered because protocol backfill depends on the historical
session/readiness tables plus repository identity; `0015_clarification_runtime.sql`
extends execution sessions and adds clarification activity storage;
`0016_execution_retry_authority.sql` adds durable attempt accounting and retry
state; `0017_runtime_event_retention.sql` adds activity expiry;
`0018_public_endpoint_abuse_protection.sql` adds the legacy-compatible setup
CIDR quota key and partial index; and `0019_otp_hmac_hardening.sql` consumes
active pre-HMAC verification codes before keyed OTP traffic is served. Never
insert a new migration into the 0001–0019 history.
