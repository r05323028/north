# OTP At-Rest Hardening

## Why

Active six-digit verification codes hashed with ordinary SHA-256 remain vulnerable to offline brute force if the verification-code table leaks. High-entropy session-token and daemon-credential hashing must remain unchanged.

## What Changes

- Replace database-only OTP hashing with keyed hashing using a server-side pepper and issuance context.
- Preserve short expiry, single use, supersession, cooldown, and bounded failed-attempt semantics.
- Define pepper storage, rotation, migration, and failure behavior.
- Add tests proving database contents alone cannot verify an active OTP.

## Capabilities

### New Capabilities

- `otp-at-rest-hardening`: Keyed protection for short-lived verification codes.

### Modified Capabilities

- `email-auth`: Change verification-code hashing while preserving existing authentication semantics.

## Impact

Future changes will affect `crates/north-persistence` verification-code hashing and migration logic, server secret configuration, authentication tests, and security documentation. Session-token and daemon-credential hashing are explicitly out of scope.
