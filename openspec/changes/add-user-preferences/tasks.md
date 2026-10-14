# Tasks

## 1. Persistent API and authorization

- [x] 1.1 Add the append-only SeaORM migration, entity, and persistence operations for one typed `user_requirement_preferences` row per user; verify defaults, upsert, and invalid-value tests with `NORTH_TEST_DATABASE_URL` and `./scripts/validate.sh integration`.
- [x] 1.2 Add authenticated GET/PUT for the current user's Requirement-workspace preferences, deriving owner from the session and rejecting invalid fields; add HTTP integration tests for 401, defaults, save/readback, Admin/Owner cross-user isolation, and no partial update; verify with `NORTH_TEST_DATABASE_URL` and `./scripts/validate.sh integration`.
- [x] 1.3 Update `docs/architecture/persistence.md` and `docs/product/roles-and-permissions.md` for durable personal preferences and self-only authorization; verify `git diff --check` and `./scripts/validate.sh specs`.

## 2. Workspace editor and reusable browser flow

- [x] 2.1 Load saved defaults in the Requirement workspace and List controls, add the page-specific “Save as default” action, and keep search transient; add web unit tests for defaults, restoration, and save failures; verify with `cd apps/web && npm test && npm run typecheck`.
- [x] 2.2 Add a reusable Playwright preference-editor flow with page-specific adapters and use it for the Requirement workspace; verify assembled save/reload behavior with `cd apps/web && npm run test:e2e:release` against the configured North stack (`NORTH_RELEASE_BASE_URL` plus storage-state or OTP auth variables).
- [x] 2.3 Update `docs/development/testing.md` with the reusable preference-flow coverage and assembled E2E invocation; verify `git diff --check` and `./scripts/validate.sh fast`.

## 3. Cross-layer qualification

- [x] 3.1 Run `./scripts/validate.sh fast`, `NORTH_TEST_DATABASE_URL`-configured `./scripts/validate.sh ci`, `openspec validate --all --strict`, and `./scripts/pre-push-validation.sh`; verify every command passes.
- [x] 3.2 Run assembled release E2E against the exact stack/artifacts; verify save, reload, and canonical restoration pass without weakening existing release tests.

## 4. Preference response race correction

- [x] 4.1 Guard save responses by local edit generation, prevent overlapping saves, and add deterministic unit coverage for stale load/save behavior and local-state preservation; verify with the RequirementWorkspace Vitest suite and web typecheck.
