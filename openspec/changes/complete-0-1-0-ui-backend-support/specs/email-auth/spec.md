# Spec Delta

## ADDED Requirements

### Requirement: Current user is a minimal read-only profile

The authenticated `GET /auth/me` endpoint SHALL return the current user's stable `id`, `email`, `role`, and persisted `created_at` timestamp. The response SHALL NOT expose session identifiers, session tokens, credential hashes, verification codes, or other authentication secrets. The profile endpoint SHALL be read-only; role changes SHALL remain under the existing administrator-gated role-management API.

#### Scenario: Authenticated profile contains canonical identity

- **WHEN** an authenticated user requests `GET /auth/me`
- **THEN** the JSON response contains that user's id, email, role, and creation timestamp from the persisted user record

#### Scenario: Profile response exposes no session secret

- **WHEN** an authenticated user requests `GET /auth/me`
- **THEN** the response contains no session token, session hash, verification code, or credential field

#### Scenario: Unauthenticated profile request is refused

- **WHEN** a request without a valid session calls `GET /auth/me`
- **THEN** the server returns HTTP 401

#### Scenario: Profile cannot change role

- **WHEN** a user attempts to mutate role through the current-user profile endpoint
- **THEN** the mutation is refused and role assignment remains available only through the existing administrator-gated endpoint
