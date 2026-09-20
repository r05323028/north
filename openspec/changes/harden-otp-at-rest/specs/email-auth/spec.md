## MODIFIED Requirements

### Requirement: Verification code issuance

The system SHALL accept an email address and issue a short-lived, single-use verification code for it through the configured CodeDelivery sink in 0.1.0; the default LogCodeDelivery sink emits that development/self-hosted delivery output through backend logs. This delivery boundary is separate from abuse-control telemetry. At most one active code SHALL exist per normalized email; requesting a new code supersedes the old. The API response MUST NOT contain the code. Each issued code SHALL have a small bounded failed-verification-attempt budget. Failed attempts SHALL be counted transactionally for that issued code; reaching the limit SHALL invalidate the code. A successful verification SHALL consume the code as before. Request-code cooldown SHALL remain independent from the verification attempt budget. The HTTP/authentication boundary SHALL apply the established `normalize_email` contract before persistence; persistence SHALL receive and bind that canonical value without a second normalization policy. Stored verification-code values SHALL use the exact keyed HMAC-SHA-256 construction and server-key configuration defined by `otp-at-rest-hardening`; session-token and daemon-credential hashing SHALL remain unchanged. If the configured OTP key is unavailable, issuance and verification SHALL fail closed rather than fall back to ordinary hashing.

#### Scenario: Code arrives via logs only

- **WHEN** a user requests a code within client and email limits
- **THEN** the configured delivery sink receives it, the code appears only in that intentional delivery output, and the HTTP response contains no trace of it

#### Scenario: Request rate and cooldown are distinct

- **WHEN** a client bucket rejects a request or the normalized email cooldown rejects a request
- **THEN** no code is created and the response is generic 429; a client-bucket rejection does not mutate email state, while a cooldown rejection consumes the client token already admitted and does not reset or replace either control

#### Scenario: Concurrent code requests stay bounded

- **WHEN** concurrent clients request codes for one normalized email
- **THEN** client controls and the existing email transaction preserve one active code/cooldown without issuing a code for rejected requests

#### Scenario: Verification budget remains separate

- **WHEN** incorrect verification submissions reach their configured limit
- **THEN** the code is invalidated under the existing transactional budget, and request-code rate limiting does not reset or replace that budget

#### Scenario: Existing issuance semantics remain

- **WHEN** a new eligible code is requested after normal cooldown
- **THEN** the old code is superseded, the new code has a fresh verification budget, and no code appears in the API response

#### Scenario: Expired or reused code is refused

- **WHEN** a user submits a code past its lifetime or already consumed
- **THEN** verification fails with a generic error and no session is created

#### Scenario: Wrong attempts consume bounded budget

- **WHEN** a user submits an incorrect code repeatedly for one issued code
- **THEN** each failed attempt is counted and the code becomes invalid after the configured small limit

#### Scenario: Correct code after failure limit is refused

- **WHEN** a user submits the correct code after that code reached its failed attempt limit
- **THEN** verification fails and no session is created

#### Scenario: New code receives fresh budget

- **WHEN** a user requests a new code after an earlier code has failed attempts
- **THEN** the earlier code is superseded and the new code starts with a fresh attempt budget subject to the normal request cooldown

#### Scenario: Concurrent failures cannot bypass limit

- **WHEN** concurrent requests submit invalid codes for the same issued code
- **THEN** no more than the configured bounded number of attempts can commit
  and the code is invalid once that limit is reached

#### Scenario: Keyed storage preserves login behavior

- **WHEN** a user submits the correct active code while the configured server key is available
- **THEN** verification succeeds with existing session and single-use semantics without changing high-entropy credential hashing

#### Scenario: Missing key fails closed

- **WHEN** the server cannot load a valid configured OTP key
- **THEN** it does not issue or verify codes and does not fall back to database-only hashing
