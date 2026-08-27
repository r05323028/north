## MODIFIED Requirements

### Requirement: Verification code issuance

The system SHALL accept an email address and issue a short-lived, single-use verification code for it, delivered through backend logs in 0.1.0. At most one active code SHALL exist per email; requesting a new code supersedes the old. The API response MUST NOT contain the code. Each issued code SHALL have a small bounded failed-verification-attempt budget. Failed attempts SHALL be counted transactionally for that issued code; reaching the limit SHALL invalidate the code. A successful verification SHALL consume the code as before. Request-code cooldown SHALL remain independent from the verification attempt budget. Stored verification-code values SHALL use keyed hashing with a server-held secret and issuance context; session-token and daemon-credential hashing SHALL remain unchanged.

#### Scenario: Code arrives via logs only

- **WHEN** a user requests a code for <operator@example.com>
- **THEN** the code appears in server logs and the HTTP response contains no trace of it

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
- **THEN** no more than the configured bounded number of attempts can commit and the code is invalid once that limit is reached

#### Scenario: Keyed storage preserves login behavior

- **WHEN** a user submits the correct active code while the server-held OTP secret is available
- **THEN** verification succeeds with existing session and single-use semantics without changing high-entropy credential hashing
