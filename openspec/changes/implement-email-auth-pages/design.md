# Design

## Context

See `proposal.md` and the `email-auth` delta. The existing web API helper sends same-origin JSON requests with credentials and accepts empty successful responses. `NorthShell` currently owns workspace navigation and fetches `/auth/me`; it has no auth-route exception or guest redirect. Auth endpoints are public and backend remains session/role authority.

## Goals / Non-Goals

**Goals:** Match OpenDesign login/signup screens, reuse one OTP flow, preserve the server-issued HttpOnly session, route guests to login, and keep workspace navigation off auth screens.

**Non-Goals:** Change Rust auth behavior, delivery providers, session policy, or authorization rules.

## Decisions

- Use one client `AuthPage` parameterized by `login` or `signup`, with thin App Router pages. Shared forms keep API payloads, validation, resend, and error handling aligned; two copied forms would drift.
- Use the existing `requestJson` helper for both POSTs. It already includes credentials, serializes the same-origin JSON contract, handles 204 responses, and exposes status without requiring a Next Server Action or new proxy.
- Move current-user loading to `NorthShell`; omit the shell on `/login` and `/signup`, and replace the current URL with `/login` only for `ApiError(401)`. Other failures remain visible as unavailable-user state instead of being misclassified as unauthenticated.
- Use an auth CSS module backed by existing global color tokens and theme bootstrap. Keep delivery copy configurable-neutral because 0.1.0 defaults to `LogCodeDelivery`.
- Exercise UI states with endpoint stubs and expose an opt-in live backend browser test through a Playwright route proxy. The live test reads OTP only from an explicitly configured private log, checks session-cookie flags and `/auth/me` cookie forwarding, and disables traces. Run it only against a fresh disposable database.

## Risks / Trade-offs

- Default OTP codes appear in server logs, not an inbox → tell operators and users to use configured delivery guidance; do not imply SMTP.
- Live browser probe creates persistent user rows → require a fresh disposable database and private log; never run against production.
- Guest redirect is a client-side UX boundary only → protected backend routes remain the security authority.

## Migration Plan

No data or API migration. Deploy frontend with the existing auth backend; rollback is reverting the web change.
