# OTP at-rest hardening implementation tasks

## 1. Configuration and secret loading

- [x] 1.1 Add an OTP key type with strict `NORTH_OTP_HMAC_KEY` parsing: exactly 64 ASCII hex characters representing 32 bytes; reject missing, malformed, odd-length, and short values without echoing secret material.
- [x] 1.2 Thread an explicit parsed OTP key through `AuthStore` constructors and server `build_app*` startup wiring; make missing/invalid production configuration fail before routes are served, with no default or unkeyed fallback.
- [x] 1.3 Update all direct persistence/router fixtures to supply an explicit deterministic non-production test key; add an explicit environment-backed test for startup missing/invalid-key failures.
- [x] 1.4 Validate configuration behavior with `cargo test -p north-persistence otp_key` and `cargo test -p north-server auth`.

## 2. Digest construction and domain model

- [x] 2.1 Add the OTP-only HMAC-SHA-256 helper using domain ASCII bytes `north/otp/verification-code/v1` followed by one NUL byte (`0x00`), length-prefixed email already canonicalized by the authentication boundary, big-endian verification row ID, and length-prefixed unmodified code bytes; the helper adds no second email-normalization policy.
- [x] 2.2 Reserve the existing `verification_codes.id` sequence value inside the issuance transaction before computing the MAC; store raw 32-byte output in existing `code_hash BYTEA`.
- [x] 2.3 Compare candidate and stored MACs with fixed-length constant-time equality; keep `hash_secret` separate and unchanged.
- [x] 2.4 Add deterministic HMAC test vectors covering canonical boundary input, framing, output bytes, different issuance IDs/emails, and changed domain/context inputs.
- [x] 2.5 Validate the helper with `cargo test -p north-persistence otp_digest` and `cargo fmt --all -- --check`.

## 3. Persistence and schema migration

- [x] 3.1 Add the next versioned migration that marks all pre-existing active `verification_codes` rows used before the HMAC path accepts traffic; preserve rows and existing indexes.
- [x] 3.2 Keep issuance transaction order and per-email cooldown/supersession semantics while inserting the explicitly reserved row ID and HMAC.
- [x] 3.3 Keep verification row locking, expiry/use predicates, failed-attempt increments, max-attempt invalidation, user bootstrap, and session creation unchanged apart from candidate MAC construction.
- [x] 3.4 Add PostgreSQL migration tests proving legacy active rows cannot verify after migration and new rows contain only fixed-length binary MAC data.
- [x] 3.5 Validate migration behavior with `NORTH_TEST_DATABASE_URL="$NORTH_TEST_DATABASE_URL" ./scripts/validate.sh integration` when an isolated PostgreSQL database is available.

## 4. Issuance path

- [x] 4.1 Compute the MAC only after the email cooldown/advisory-lock checks pass and the new verification row ID is reserved; do not persist plaintext code or MAC input.
- [x] 4.2 Preserve one-active-code supersession, ten-minute expiry, one-minute request cooldown, fresh failed-attempt budget, and configured `CodeDelivery` behavior.
- [x] 4.3 Add an issuance integration assertion that two equal codes in different issuance contexts produce different stored MACs and that API responses remain code-free.
- [x] 4.4 Validate issuance with the focused `north-persistence` and `north-server` authentication tests plus `cargo test --workspace`.

## 5. Verification path

- [x] 5.1 Recompute the HMAC from the selected row's email canonicalized at the authentication boundary, row ID, submitted code, and active key before the existing constant-time comparison.
- [x] 5.2 Preserve generic invalid-code behavior, expiry rejection, single-use consumption, transactional failed-attempt counting, and the existing attempt limit under concurrent verification.
- [x] 5.3 Add tests for correct and incorrect codes, expired/reused codes, superseded codes, concurrent failures, and correct-code rejection after the attempt limit.
- [x] 5.4 Validate verification with `cargo test -p north-persistence verification` and the relevant `north-server` auth/integration tests.

## 6. Rotation and legacy behavior

- [x] 6.1 Implement one-active-key semantics: changing `NORTH_OTP_HMAC_KEY` accepts no previous key and invalidates all outstanding old-key OTPs; do not add key IDs or a previous-key grace list.
- [x] 6.2 Test missing/invalid key startup, legacy SHA-256 rejection, key-change rejection of old codes, and successful issuance/verification under the new key.
- [x] 6.3 Document coordinated key rollout, reissue behavior, and forward-only rollback after the legacy invalidation migration.
- [x] 6.4 Validate rotation and migration cases with `NORTH_TEST_DATABASE_URL` integration tests and `openspec validate --all --strict`.

## 7. Security and regression tests

- [x] 7.1 Prove persisted verification data contains no plaintext OTP, key, or MAC input and that non-delivery logs/errors/telemetry/traces contain no OTP or key material; keep the intentional `CodeDelivery` boundary explicit.
- [x] 7.2 Add regression coverage proving session-token, daemon setup-token, daemon-credential, and other unrelated hashing remains the existing `hash_secret` behavior.
- [x] 7.3 Add a read-only-database security test/fixture showing stored values cannot validate guessed OTPs without the server key.
- [x] 7.4 Validate security regressions with `cargo test --workspace` and the focused PostgreSQL suites when their environment prerequisite is available.

## 8. Documentation and configuration examples

- [x] 8.1 Document `NORTH_OTP_HMAC_KEY`, strict 32-byte hex format, startup failure, explicit test keys, rotation invalidation, and legacy migration behavior in the canonical configuration/security documentation.
- [x] 8.2 Update the active `email-auth` and `otp-at-rest-hardening` delta specs while preserving the daemon/session hashing boundary and existing delivery semantics; defer canonical spec sync/materialization until this change is archived.
- [x] 8.3 Validate documentation and OpenSpec artifacts with `openspec validate --all --strict` and the repository's documentation/format checks.

## 9. Final validation and cleanup

- [x] 9.1 Review the diff for runtime changes outside OTP issuance/verification, especially session tokens, daemon credentials, setup tokens, API credentials, and password-like secrets.
- [x] 9.2 Run `./scripts/validate.sh fast` and record any environment-dependent PostgreSQL omissions truthfully; run the full integration profile when `NORTH_TEST_DATABASE_URL` is available.
- [x] 9.3 Confirm all task checkboxes, strict OpenSpec validation, focused tests, and relevant documentation checks are green before implementation completion.
