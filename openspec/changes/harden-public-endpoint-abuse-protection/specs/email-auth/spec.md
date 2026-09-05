## MODIFIED Requirements

### Requirement: Verification code issuance

The system SHALL accept an email address and issue a short-lived, single-use
verification code for it, delivered through backend logs in 0.1.0. At most one
active code SHALL exist per normalized email; requesting a new code supersedes
the old. The API response MUST NOT contain the code. Each issued code SHALL
have a small bounded failed-verification-attempt budget. Failed attempts SHALL
be counted transactionally for that issued code; reaching the limit SHALL
invalidate the code. A successful verification SHALL consume the code as
before.

A syntactically valid request to `POST /auth/request-code` SHALL pass the
separate process-local client/network bucket and the existing normalized-email
cooldown/resource transaction. Client request rate, email cooldown, and failed
verification-attempt budget are independent controls and SHALL NOT be merged.
Any rate/cooldown rejection SHALL create no code and use the canonical generic
HTTP 429 `rate_limited` response with safe `Retry-After` where applicable. The
response SHALL not reveal whether an email has an active code or account.

#### Scenario: Code arrives via logs only

- **WHEN** a user requests a code within client and email limits
- **THEN** the code appears in server logs and the HTTP response contains no
  trace of it

#### Scenario: Request rate and cooldown are distinct

- **WHEN** a client bucket rejects a request or the normalized email cooldown
  rejects a request
- **THEN** no code is created, the response is generic 429, and exhausting one
  control does not mutate or consume the other control's state

#### Scenario: Concurrent code requests stay bounded

- **WHEN** concurrent clients request codes for one normalized email
- **THEN** client controls and the existing email transaction preserve one
  active code/cooldown without issuing a code for rejected requests

#### Scenario: Verification budget remains separate

- **WHEN** incorrect verification submissions reach their configured limit
- **THEN** the code is invalidated under the existing transactional budget, and
  request-code rate limiting does not reset or replace that budget

#### Scenario: Existing issuance semantics remain

- **WHEN** a new eligible code is requested after normal cooldown
- **THEN** the old code is superseded, the new code has a fresh verification
  budget, and no code appears in the API response

#### Scenario: Expired or reused code is refused

- **WHEN** a user submits a code past its lifetime or already consumed
- **THEN** verification fails with a generic error and no session is created

#### Scenario: Wrong attempts consume bounded budget

- **WHEN** a user submits an incorrect code repeatedly for one issued code
- **THEN** each failed attempt is counted and the code becomes invalid after the
  configured small limit

#### Scenario: Correct code after failure limit is refused

- **WHEN** a user submits the correct code after that code reached its failed
  attempt limit
- **THEN** verification fails and no session is created

#### Scenario: New code receives fresh budget

- **WHEN** a user requests a new code after an earlier code has failed attempts
- **THEN** the earlier code is superseded and the new code starts with a fresh
  attempt budget subject to the normal request cooldown

#### Scenario: Concurrent failures cannot bypass limit

- **WHEN** concurrent requests submit invalid codes for the same issued code
- **THEN** no more than the configured bounded number of attempts can commit
  and the code is invalid once that limit is reached
