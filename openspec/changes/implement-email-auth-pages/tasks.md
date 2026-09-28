# Tasks

## 1. Email login and signup screens

- [x] 1.1 Add shared responsive `/login` and `/signup` pages from the OpenDesign screens, with auth routes outside workspace shell; verify direct route rendering, cross-links, mobile layout, and theme behavior in web tests.
- [x] 1.2 Integrate both pages with existing request-code and verify endpoints, including validation, generic 401/429 errors, resend/change-email, session-cookie navigation; verify stubbed login/signup flows and add opt-in live backend coverage for cookie flags and `/auth/me` session recognition.
- [x] 1.3 Update `docs/deployment/self-hosted.md` and `docs/development/web.md` with browser login/signup entry, server-controlled first Owner, configured OTP delivery, and the disposable live-probe command; verify guidance matches the default log sink.

## 2. Guest workspace routing

- [x] 2.1 Move current-user lookup into `NorthShell` and redirect workspace routes to `/login` only on `/auth/me` 401; add Playwright coverage for guest redirect, auth-route bypass, and non-401 behavior.

## 3. Integration validation

- [x] 3.1 Run web lint, typecheck, unit tests, stubbed and disposable live auth Playwright tests, `./scripts/validate.sh fast`, and `openspec validate --all --strict`; report unavailable environment-dependent checks explicitly.
- [x] 3.2 Review final diff and actual Git changed-file set; verify only requested auth UI, tests, canonical docs, and this OpenSpec change were added beyond pre-existing work.
