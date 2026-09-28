# Proposal

## Why

North has a passwordless email-code backend but no browser sign-in or account-creation screens. Visitors without a session currently reach protected workspace UI without a clear authentication path.

## What Changes

- Add `/login` and `/signup` pages based on OpenDesign auth screens, with responsive layout, theme support, accessible validation, code resend, and email-edit flow.
- Wire both pages to existing `POST /auth/request-code` and `POST /auth/verify`; successful verification relies on the backend session cookie and enters the workspace.
- Add an opt-in live browser probe for the real auth endpoints and session cookie, restricted to a disposable database and private code-delivery log.
- Keep auth pages outside the workspace shell. Redirect workspace visitors to `/login` only when `/auth/me` returns 401.
- Document browser entry and first-owner behavior without implying SMTP delivery; configured `CodeDelivery` remains authoritative (default: backend logs).

Out of scope: auth API or persistence changes, SMTP/provider setup, passwords, social login, logout UI, and authorization-policy changes.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `email-auth`: specify the browser login/signup flow and unauthenticated workspace entry behavior.

## Impact

- `apps/web/app`, `apps/web/components/north-shell.tsx`, and web browser tests.
- Existing North auth endpoints and session cookie; no backend contract or dependency changes.
- Canonical operator guidance: `docs/deployment/self-hosted.md`; live browser-probe instructions: `docs/development/web.md`.
