## Purpose

Protects short-lived low-entropy verification codes from database-only offline recovery while preserving existing login semantics and high-entropy credential hashing.

## ADDED Requirements

### Requirement: Verification codes use keyed at-rest protection

The system SHALL derive stored verification-code values with a server-held secret and issuance context, such that database contents alone are insufficient to validate an active six-digit code. The keyed construction SHALL cover normalized email, issuance identity, and code, and SHALL define secret configuration and rotation behavior. Session-token and daemon-credential hashing SHALL remain unchanged by this capability.

#### Scenario: Database contents alone cannot validate an active code

- **WHEN** an attacker obtains the verification-code table without the server-held secret
- **THEN** the attacker cannot validate candidate active codes using the stored values alone

#### Scenario: Correct code still verifies

- **WHEN** a user submits the correct active code through the normal login flow
- **THEN** verification succeeds with existing expiry, single-use, cooldown, supersession, and failed-attempt semantics

#### Scenario: Secret configuration is required

- **WHEN** the server cannot load its configured OTP hashing secret
- **THEN** code issuance and verification fail safely rather than falling back to database-only hashing
