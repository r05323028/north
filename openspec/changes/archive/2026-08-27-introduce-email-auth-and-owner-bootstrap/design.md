# Design

## Context

First server-side feature. Persistence, HTTP host, and session handling start here.

## Decisions

- **Database**: PostgreSQL via `sqlx` in `crates/north-persistence`; migrations
  live in `/migrations` and are applied at startup by the server.
- **Owner bootstrap**: a singleton `instance_settings` row holds
  `owner_user_id`. Signup claims it inside the same transaction that inserts
  the user (`UPDATE instance_settings SET owner_user_id = $new WHERE id = 1
  AND owner_user_id IS NULL`; claim winner inserts with role Owner, losers as
  Requester). Uniqueness + transactional update make double-claim impossible.
- **Code delivery**: `CodeDelivery` trait (`send(email, code)`); only a
  log-backed implementation ships. Adding a provider touches composition, not
  auth semantics.
- **Sessions**: opaque random token, HTTP-only Secure cookie, server-side
  session table with expiry.
- Codes stored hashed with expiry and used-at; single active code per email;
  constant-time comparison.

## Open Questions

- Exact code lifetime and request rate limits — config constants, not spec.
