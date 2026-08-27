# email-auth Specification

## Purpose

Lets people sign in to a self-hosted North with nothing but the instance URL
and an email address, using short-lived verification codes emitted to backend
logs, while guaranteeing exactly one initial Owner per fresh instance.

## Requirements

### Requirement: Verification code issuance

The system SHALL accept an email address and issue a short-lived, single-use
verification code for it, delivered through backend logs in 0.1.0. At most one
active code SHALL exist per email; requesting a new code supersedes the old.
The API response MUST NOT contain the code. Each issued code SHALL have a
small bounded failed-verification-attempt budget. Failed attempts SHALL be
counted transactionally for that issued code; reaching the limit SHALL
invalidate the code. A successful verification SHALL consume the code as
before. Request-code cooldown SHALL remain independent from the verification
attempt budget.

#### Scenario: Code arrives via logs only

- **WHEN** a user requests a code for <operator@example.com>
- **THEN** the code appears in server logs and the HTTP response contains no
trace of it

#### Scenario: Expired or reused code is refused

- **WHEN** a user submits a code past its lifetime or already consumed
- **THEN** verification fails with a generic error and no session is created

#### Scenario: Wrong attempts consume bounded budget

- **WHEN** a user submits an incorrect code repeatedly for one issued code
- **THEN** each failed attempt is counted and the code becomes invalid after
  the configured small limit

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

### Requirement: Session establishment

Successful verification SHALL create a durable user record and an
HTTP-only, Secure session cookie. Sessions SHALL be server-tracked with expiry
and support logout/invalidation. Normal API responses MUST NOT expose secrets
(codes, session tokens, hashes).

#### Scenario: Login yields usable session

- **WHEN** a correct code is verified
- **THEN** subsequent authenticated requests identify the new user until
logout or expiry

#### Scenario: Logout invalidates server-side

- **WHEN** a logged-in user logs out
- **THEN** the session token stops working even if the cookie was copied

### Requirement: Atomic first-owner bootstrap

On a fresh instance the first successfully created account SHALL become Owner
via an atomic database operation. Two concurrent first sign-ups SHALL yield
exactly one Owner and one ordinary account. Later accounts SHALL be created as
Requester.

#### Scenario: Race produces a single Owner

- **WHEN** two sign-ups commit simultaneously on an empty instance
- **THEN** exactly one transaction wins the owner claim; the loser persists as
a normal non-Owner account

### Requirement: Pluggable code delivery

Verification-code sending SHALL sit behind an internal delivery boundary so
adding an email provider does not change issuance/verification behavior,
code lifetime semantics, or storage layout.

#### Scenario: Delivery swap leaves auth intact

- **WHEN** a provider other than log-emission is configured
- **THEN** codes verify identically with unchanged endpoints and tables
