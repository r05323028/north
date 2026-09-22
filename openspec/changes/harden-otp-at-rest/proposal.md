# OTP At-Rest Hardening

## Why

Active six-digit verification codes currently use ordinary SHA-256. A database-only reader can therefore test guesses without server-held material. Session-token and daemon-credential hashing already protect high-entropy values and must not be changed by this follow-up.

## What Changes

- Store verification-code digests as keyed HMAC-SHA-256 values bound to normalized email, the existing verification row ID, and an OTP-specific domain separator.
- Load one strictly validated 256-bit server key from `NORTH_OTP_HMAC_KEY`; missing or invalid startup configuration fails closed and no unkeyed fallback exists.
- Invalidate active legacy SHA-256 rows in a migration; never verify legacy digests or accept previous rotation keys.
- Define explicit development/test key injection, constant-time comparison, redacted error/log behavior, and key-change invalidation semantics.
- Preserve delivery, expiry, supersession, cooldown, bounded failed attempts, single-use sessions, and existing authentication responses.

## Capabilities

### New Capabilities

- `otp-at-rest-hardening`: Keyed protection for short-lived verification codes.

### Modified Capabilities

- `email-auth`: Change verification-code hashing while preserving existing authentication semantics.

## Impact

Future implementation changes affect `crates/north-persistence` verification-code hashing and migration logic, server startup secret configuration, authentication tests, and security documentation. The existing `CodeDelivery` boundary remains; its intentional development/self-hosted delivery output is not abuse-control telemetry. Session-token, daemon-credential, setup-token, API-credential, and password-like hashing remain outside this change.

## Explicit Boundaries

This change applies only to the `verification_codes` issuance and verification flow. The shared high-entropy `hash_secret` behavior used for sessions and daemon credentials MUST remain unchanged. No general-purpose secret-management or KMS abstraction is introduced.
