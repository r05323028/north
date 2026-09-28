# Web development

## Next.js documentation

For changes under `apps/web/`, read the relevant guide from the installed Next.js
package at `apps/web/node_modules/next/dist/docs/` before writing code. In this
monorepo, resolve the package from `apps/web/`; it may not be visible from the
repository root. APIs, conventions, and file structure can differ from other
Next.js versions. Follow the installed guide and heed deprecation notices.

## Generated agent guidance

`apps/web/AGENTS.md` contains a Next.js-managed rules block. `next dev` may
update or recreate that block when it detects an AI coding agent. Generator:
`apps/web/node_modules/next/dist/server/lib/generate-agent-files.js`.
Keep the managed block in place; North-specific durable guidance belongs in the
root `AGENTS.md` and this document. If both app-local `AGENTS.md` and
`CLAUDE.md` are absent, the generator scaffolds both.

## Optional live auth browser probe

`apps/web/e2e/auth-live.spec.ts` exercises signup and login against a running
North server. Use only a fresh, disposable PostgreSQL database: the probe
creates two users and expects first-user Owner bootstrap. It forwards browser
`/auth/**` requests to the configured backend, checks the real `Secure`,
`HttpOnly`, and `SameSite=Lax` session cookie, then verifies `/auth/me` receives
that cookie and the workspace recognizes each account.

The default `LogCodeDelivery` writes verification codes and addresses to server
logs. Keep that log private and use a mode-600 file; the test reads codes from
that configured sink but does not print them or retain Playwright traces. Start
the server with its disposable database and OTP key using the self-hosted
procedure, redirect its output to a private log, then run from `apps/web`:

```bash
NORTH_AUTH_TEST_BACKEND_URL=http://127.0.0.1:8080 \
NORTH_AUTH_TEST_LOG=/path/to/private/north-server.log \
npm run test:e2e -- auth-live.spec.ts
```

Without both variables the live test skips; regular stubbed auth browser tests
remain runnable with `npm run test:e2e -- auth.spec.ts`. Never point this probe
or its log at production.
