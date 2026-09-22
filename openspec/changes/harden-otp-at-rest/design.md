# Design: keyed OTP at-rest protection

## Context

North currently generates six-digit codes in `crates/north-server/src/auth.rs` and stores/verifies them through `AuthStore::issue_code` and `AuthStore::verify_code` in `crates/north-persistence/src/lib.rs`. The persistence helper `hash_secret` is ordinary SHA-256 and is also used for session tokens, daemon setup request tokens, and daemon credentials. The verification row already has a positive `BIGSERIAL id`, normalized email, binary `code_hash`, expiry, use state, and failed-attempt count. Authentication startup runs embedded SQL migrations before routes are served. North has no separate production server binary or general secret-management layer; `build_app*` is the server startup boundary.

The existing `CodeDelivery` trait receives the plaintext code. `LogCodeDelivery` intentionally emits development/self-hosted delivery output through backend logs. That delivery behavior is separate from abuse-control telemetry and is not changed by this at-rest design.

## Goals / Non-Goals

**Goals:**

- Make a read-only verification-code table insufficient for efficient offline guessing without the server-held key.
- Bind every MAC to OTP purpose, canonical email, issuance row ID, and exact code bytes.
- Fail closed on missing/invalid configuration, legacy records, and key replacement.
- Preserve current expiry, cooldown, supersession, single-use, failed-attempt, session, delivery, and generic-error behavior.
- Keep the implementation small enough for North's current single-server/self-hosted maturity.

**Non-goals:**

- Changing session-token, daemon-token, daemon-credential, API-credential, password-like, protocol-digest, or other non-OTP hashing.
- Adding a general KMS, secret-management service, rotation coordinator, previous-key grace window, or HA key-distribution protocol.
- Replacing the configured code-delivery channel.
- Making HMAC a slow password KDF; the server-held key, expiry, and existing attempt budget are the controls for this short-lived OTP flow.

## Decisions

### 1. Explicit server key and fail-closed startup

Add a small OTP key type owned by the persistence/auth boundary. `NORTH_OTP_HMAC_KEY` is the only production configuration source. It is strict ASCII hexadecimal for exactly 32 bytes (64 hex characters). Parsing rejects missing, odd-length, non-hex, or shorter-than-256-bit values without echoing the value. The parsed key is held in `AuthStore` and is never formatted with `Debug` or included in an error.

`build_app` and its configurable variants load and validate the environment key before serving routes, then pass the parsed key into `AuthStore`. Persistence constructors require an explicit parsed key (`AuthStore::new(pool, key)` and the retention equivalent); they do not read the environment or synthesize a default. This keeps the production boundary fail-closed and makes test setup explicit. Unit/integration fixtures use a fixed non-production test key through the same constructor. A test that exercises startup configuration sets or removes `NORTH_OTP_HMAC_KEY` explicitly.

**Alternative rejected:** looking up the environment on each issuance/verification would make runtime failure timing unclear and would force secret access into the persistence operation. A development fallback would recreate the database-only weakness and is forbidden.

### 2. HMAC-SHA-256 with framed, domain-separated input

Use the existing `hmac`/`sha2` ecosystem and `Hmac<Sha256>`. Keep `hash_secret` unchanged and add an OTP-only helper. The HTTP/authentication boundary owns the existing `normalize_email` contract: trim surrounding Unicode whitespace with `str::trim`, then ASCII lowercase, with no Unicode normalization. It passes that canonical value into persistence; the HMAC helper consumes it without applying a second normalization policy. Build the MAC message exactly as:

```text
ASCII bytes: north/otp/verification-code/v1
NUL byte: 0x00
u32 big-endian: canonical email UTF-8 byte length
bytes: canonical email already normalized by the authentication boundary
u64 big-endian: verification_codes.id
u32 big-endian: code UTF-8 byte length
bytes: code, with no trimming or transformation
```

The row ID is allocated from the existing `verification_codes_id_seq` inside the issuance transaction before computing the MAC, then inserted explicitly with the MAC. Sequence gaps on rollback are harmless. Verification selects the row ID and recomputes the same frame. The output is the 32 raw HMAC bytes in the existing `code_hash BYTEA` column; no hex/base64 conversion is used. Compare the fixed-length values with `subtle::ConstantTimeEq`.

Length-prefixing prevents ambiguous concatenation. The fixed domain separator prevents this MAC from being reused as a credential/session or other-purpose digest. Including row ID means the same code issued twice cannot reuse stored verification material, even for one email.

**Alternatives rejected:** plain SHA-256 remains offline-brute-forceable; HMAC over only email+code permits cross-issuance reuse; random salt without the existing row ID would require another persisted field; a password KDF is unnecessary for this short-lived, attempt-bounded flow.

### 3. Persistence and legacy migration

Add one versioned SQL migration after the current migrations. It sets `used_at = CURRENT_TIMESTAMP` only for `verification_codes` rows where `used_at IS NULL AND expires_at > CURRENT_TIMESTAMP`, preserving already-used and expired-unused rows for normal retention while invalidating active pre-deployment unkeyed digests. Expired legacy rows remain unused, but `verify_code` still rejects them because verification requires `expires_at > CURRENT_TIMESTAMP`. Because startup applies migrations before accepting requests, no legacy active code is admitted into the new path. No schema type change is needed; `code_hash` remains binary storage for the HMAC output.

`issue_code` keeps its per-email advisory lock, cooldown check, active-row supersession, and transaction. It reserves the row ID, computes the HMAC, and inserts the row. `verify_code` keeps its row lock, expiry/use predicate, failed-attempt update, max-attempt invalidation, user/owner/session transaction, and generic `InvalidCode` result; only candidate digest construction changes.

No legacy SHA-256 fallback exists. North intentionally has no previous-key verification list or stored key ID. Changing the configured key makes all old-key rows fail the MAC; they can age out without a cleanup-specific behavior. Operators must deploy the same new key to every server instance and expect outstanding codes to require reissue.

### 4. Strict separation from other credential hashing

Do not modify `hash_secret`, its SHA-256 output, or its call sites for sessions, daemon setup request tokens, daemon credentials, and session invalidation. OTP MAC construction lives in a separately named helper and takes an explicit OTP key plus OTP context. No protocol DTO, daemon path, setup-token path, or password-like path receives the OTP key.

### 5. Plaintext and observability boundary

The OTP key, MAC input, and MAC output are never logged or returned. Errors expose only generic configuration/verification status. The plaintext code exists only long enough for normal request handling and the existing `CodeDelivery::send` call. `LogCodeDelivery` remains the explicit development/self-hosted delivery exception already documented by `email-auth`; every other application log, error, metric, and trace path must remain code/key-free.

## Risks / Trade-offs

- **Key replacement invalidates outstanding OTPs.** → Use one clearly provisioned key on all instances and document reissue behavior; no unsafe previous-key grace window is added.
- **Legacy migration invalidates active login attempts.** → Apply migration before serving traffic, preserve generic errors, and require users to request a fresh code.
- **HMAC is not a slow password hash.** → The key prevents database-only verification; short TTL, supersession, and the existing five-attempt budget remain active. Password hashing is outside scope.
- **Development log delivery still contains the code by design.** → Keep it behind `CodeDelivery`, exclude it from abuse telemetry, and never log the key/MAC or duplicate the code in other paths.
- **All server instances need identical configuration.** → Startup fails closed on missing/invalid keys; deployment validation must check the environment before admitting traffic.

## Migration Plan

1. Provision a random 32-byte value as strict hex in `NORTH_OTP_HMAC_KEY` for every server instance. Do not commit or print it.
2. Deploy the implementation with the new migration. Startup validates the key, applies the migration, and marks all outstanding legacy active rows used before exposing auth routes.
3. Require new code issuance after rollout. Verify that delivery still reaches the configured sink and that session, expiry, cooldown, supersession, and attempt behavior remain unchanged.
4. For rotation, change the environment value on every instance during one controlled rollout. The first process using the new key makes all old-key OTPs fail; no previous key is accepted.
5. After the normal TTL/retention window, legacy/old-key rows may be removed by existing retention or ordinary database maintenance.

Rollback after the HMAC migration is forward-only at the application level: the pre-change binary cannot verify new HMAC rows, and the new binary cannot verify old SHA-256 rows. If emergency rollback is required after migration, restore a coordinated database backup from before the migration and run the old binary in an isolated maintenance window; do not mix old and new binaries against one live database.

## Open Questions

None. Key format, startup failure, rotation, legacy invalidation, framing, scope boundaries, and test configuration are fixed above.
