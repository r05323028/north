# otp-at-rest-hardening Specification Delta

## Purpose

Protects short-lived verification codes from database-only offline guessing with a precisely framed keyed digest while preserving North's existing authentication and credential boundaries.

## ADDED Requirements

### Requirement: Verification-code digest construction

For every issued verification code, the server SHALL store a raw 32-byte HMAC-SHA-256 output under the existing verification-code digest field. The HMAC key SHALL be the active server-held OTP key. The MAC message SHALL be the following exact byte sequence, in this order:

1. the ASCII bytes `north/otp/verification-code/v1`, followed by one NUL byte (`0x00`);
2. a four-byte unsigned big-endian email byte length;
3. the UTF-8 bytes of the canonical email. The HTTP/authentication boundary SHALL apply the existing `normalize_email` contract—trim surrounding Unicode whitespace with `str::trim`, then ASCII lowercase, with no Unicode normalization—before passing the value into persistence; the HMAC helper SHALL consume that value without additional normalization;
4. the verification row's positive `BIGSERIAL` `id` encoded as an eight-byte unsigned big-endian integer;
5. a four-byte unsigned big-endian code byte length; and
6. the UTF-8 bytes of the code, with no trimming or other transformation. Normal endpoint codes remain six ASCII digits.

Length prefixes SHALL be included exactly as stated; ambiguous concatenation and reuse across OTP purposes are forbidden. The stored output SHALL remain binary `BYTEA`, not hex or base64. Verification SHALL recompute the MAC with the row's email and ID and compare fixed-length outputs with a constant-time equality operation.

#### Scenario: Issuance context prevents reuse

- **WHEN** the same six-digit code is issued for different emails or different verification row IDs
- **THEN** the stored MAC inputs differ and a MAC from one issuance context cannot verify another context

#### Scenario: Matching context verifies

- **WHEN** a submitted code, canonical email, row ID, and active key match an unconsumed unexpired row
- **THEN** the recomputed HMAC matches the stored 32-byte value and normal verification continues

#### Scenario: Persisted data contains no plaintext code

- **WHEN** a verification row is inspected after issuance
- **THEN** it contains the binary fixed-length MAC and metadata but no plaintext OTP value

### Requirement: OTP key configuration fails closed

The server SHALL load the active OTP key from `NORTH_OTP_HMAC_KEY` before serving authentication routes. The value SHALL be strict ASCII hexadecimal representing exactly 32 bytes (at least 256 bits); missing values, odd-length values, non-hex values, or values shorter than 64 hex characters SHALL be invalid. The key value SHALL never be logged, returned, or included in errors, telemetry, or traces. Production and development startup SHALL fail closed before accepting requests when the variable is missing or invalid. There SHALL be no implicit development key, unkeyed fallback, or key derivation from `DATABASE_URL` or another application value. Tests SHALL inject an explicit deterministic test key through the key/configuration constructor or an explicitly configured test environment.

#### Scenario: Missing or invalid server key stops startup

- **WHEN** `NORTH_OTP_HMAC_KEY` is missing or fails strict validation
- **THEN** server startup returns a configuration failure before authentication routes are served and no SHA-256-only OTP path is enabled

#### Scenario: Explicit test key is required

- **WHEN** a unit or integration test constructs authentication persistence
- **THEN** it supplies a deterministic test key explicitly rather than relying on a hidden default or production secret

### Requirement: Legacy records and key rotation are fail-closed

The deployment migration SHALL invalidate every existing active verification row produced by the legacy unkeyed SHA-256 scheme before the new authentication path accepts traffic. The new implementation SHALL never attempt legacy SHA-256 verification. North SHALL use one active key and SHALL accept no previous key; replacing `NORTH_OTP_HMAC_KEY` therefore invalidates all outstanding rows made with the previous key. Those rows may remain for normal expiry/retention but SHALL never create a session. New codes issued with the new key remain usable under normal semantics.

#### Scenario: Legacy active code is rejected after migration

- **WHEN** an OTP row created before the HMAC deployment is submitted after the migration
- **THEN** the legacy code is rejected, no session is created, and no legacy digest fallback runs

#### Scenario: Key replacement invalidates outstanding codes

- **WHEN** the server key changes between issuance and verification
- **THEN** the outstanding old-key code is rejected and a newly issued code under the new key verifies normally

#### Scenario: Rotation has no hidden compatibility window

- **WHEN** operators rotate the active key
- **THEN** all server instances use the new key, no previous verification key is consulted, and operators understand that outstanding OTPs must be reissued

### Requirement: OTP security boundary preserves authentication semantics

This capability SHALL apply only to verification-code issuance and verification. It SHALL NOT change session-token, daemon setup-token, daemon-credential, API-credential, password-like, protocol-digest, or unrelated hashing behavior. Plaintext OTP values SHALL never be persisted. The OTP key and computed MAC SHALL never appear in application logs, errors, abuse telemetry, or traces. The existing configured CodeDelivery sink is the sole intentional recipient of plaintext delivery data; its development `LogCodeDelivery` output is a separate delivery boundary and remains unchanged by this at-rest capability. Expiry, supersession, cooldown, single-use consumption, transactional failed-attempt limits, and generic verification errors SHALL remain intact.

#### Scenario: Database-only reader lacks an efficient verifier

- **WHEN** an attacker has read-only access to the verification table but not the active OTP key
- **THEN** stored data alone does not provide an efficient offline check for guessed active codes

#### Scenario: Existing expiry and attempt semantics remain

- **WHEN** a code expires, is superseded, is reused, or reaches its failed-attempt limit
- **THEN** verification follows the existing rejection and transactional-consumption behavior

#### Scenario: Unrelated credential hashes remain unchanged

- **WHEN** a session token, daemon setup token, or daemon credential is issued and later verified
- **THEN** its existing `hash_secret` SHA-256 behavior and constant-time comparison remain unchanged and no OTP key/context is used

#### Scenario: Non-delivery telemetry is redacted

- **WHEN** issuance or verification emits errors, logs, metrics, or traces
- **THEN** those outputs contain no OTP, MAC, or key material; only the separately configured CodeDelivery sink may receive plaintext delivery data
