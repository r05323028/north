# Spec Delta

## ADDED Requirements

### Requirement: Browser email-code login and signup
The web app SHALL provide linked `/login` and `/signup` screens using `POST /auth/request-code` and `POST /auth/verify`; signup SHALL use shared verification and server-controlled account bootstrap without client role selection. The browser MUST NOT persist codes or inspect the HttpOnly session cookie. Delivery guidance SHALL describe configured delivery without promising unconfigured SMTP or inbox delivery.

#### Scenario: Request a code from signup
- **WHEN** a visitor submits a valid email address on `/signup` and the server returns `202 Accepted`
- **THEN** the screen advances to six-digit code entry, retains the address, and offers resend and change-email actions without revealing account existence

#### Scenario: Invalid code can be corrected or resent
- **WHEN** verification returns `401 Unauthorized` or a code request returns `429 Too Many Requests`
- **THEN** the screen shows a generic actionable error, preserves the active email, and allows code correction or resend without exposing server details

#### Scenario: Successful login establishes browser session
- **WHEN** a visitor submits a valid code on `/login` and verification returns `204 No Content` with the session cookie
- **THEN** the browser follows the cookie and navigates to `/` without reading or storing the cookie value

#### Scenario: Signup and login share backend verification
- **WHEN** a visitor switches between `/login` and `/signup`
- **THEN** both screens submit the same email/code payloads to the existing endpoints, and account role assignment remains server-controlled

#### Scenario: Code handling and delivery guidance respect browser boundaries
- **WHEN** a visitor requests or enters a verification code
- **THEN** the code stays only in active form state, and delivery guidance describes the configured method without promising unconfigured SMTP or inbox delivery

### Requirement: Unauthenticated workspace entry
The browser application SHALL redirect a visitor to `/login` when `/auth/me` returns `401 Unauthorized` on a non-auth route. `/login` and `/signup` SHALL remain accessible without a session and SHALL render without the authenticated workspace shell. Other `/auth/me` failures MUST NOT be treated as proof that the visitor is unauthenticated.

#### Scenario: Guest opens workspace
- **WHEN** a visitor opens a workspace route and `/auth/me` returns `401 Unauthorized`
- **THEN** the browser navigates to `/login`

#### Scenario: Guest opens auth screen directly
- **WHEN** a visitor opens `/login` or `/signup` without a session
- **THEN** the requested auth screen remains visible without workspace navigation

#### Scenario: Current-user endpoint is unavailable
- **WHEN** `/auth/me` fails with a non-401 response
- **THEN** the browser does not redirect to `/login` solely because of that failure
