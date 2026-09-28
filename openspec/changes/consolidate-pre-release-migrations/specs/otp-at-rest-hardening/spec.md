# Spec Delta

## MODIFIED Requirements

### Requirement: Legacy records and key rotation are fail-closed

The new implementation SHALL never attempt legacy unkeyed SHA-256 verification. Databases created by pre-0.1.0 server versions that may contain legacy verification digests are unsupported by the 0.1.0 baseline and SHALL be manually recreated before first-release use; startup SHALL NOT reset or rewrite them. North SHALL use one active key and SHALL accept no previous key; replacing `NORTH_OTP_HMAC_KEY` therefore invalidates all outstanding rows made with the previous key. Those rows may remain for normal expiry/retention but SHALL never create a session. New codes issued with the new key remain usable under normal semantics.

#### Scenario: Legacy digest has no verification fallback

- **WHEN** verification is attempted for a row containing a legacy unkeyed SHA-256 digest
- **THEN** the submitted code is rejected, no session is created, and no legacy digest fallback runs

#### Scenario: Pre-release database requires manual recreation

- **WHEN** an operator prepares a database created by a pre-0.1.0 server for first-release use
- **THEN** the database is backed up and manually recreated rather than upgraded or reset by North

#### Scenario: Legacy active code is rejected after migration

- **WHEN** an active legacy row remains in a pre-0.1.0 database presented to the 0.1.0 server
- **THEN** startup fails closed without running legacy verification, and the code cannot create a session

#### Scenario: Expired legacy record is preserved but unverifiable

- **WHEN** an expired legacy row remains in a pre-0.1.0 database presented to the 0.1.0 server
- **THEN** startup fails without resetting or rewriting the database, and the row cannot create a session

#### Scenario: Key replacement invalidates outstanding codes

- **WHEN** the server key changes between issuance and verification
- **THEN** the outstanding old-key code is rejected and a newly issued code under the new key verifies normally

#### Scenario: Rotation has no hidden compatibility window

- **WHEN** operators rotate the active key
- **THEN** all server instances use the new key, no previous verification key is consulted, and operators understand that outstanding OTPs must be reissued
