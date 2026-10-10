# Proposal

## Why

North exposes authenticated identity but has no saved personal preferences. Users need page-specific editors with distinct permission rules, without conflating personal values with shared instance settings.

## What Changes

- Add durable preferences scoped to the signed-in user and page.
- V1 covers the Requirement workspace at `/`: persist default view (`board` or `list`) and List filters/sort (status, creator, `updated`/`updated_asc`). Search text remains transient.
- Each participating page owns its own editor, typed preference fields, and authorization policy. V1 registers only Requirement workspace; later pages opt in separately rather than sharing a generic editor or role matrix.
- Enforce personal ownership at the server boundary: derive user identity from the session, allow users to read/update only their own preferences, and apply the registered page's authorization policy. Any authenticated role may edit its own Requirement workspace preferences.
- Add reusable Playwright coverage for each registered editor. V1 verifies load, save, reload, and cross-user isolation.
- Keep identity, role assignment, and shared repository/daemon settings unchanged.

V1 excludes search history, AI/notification preferences, shared instance settings, and preferences for admin/settings pages.

## Capabilities

### New Capabilities

- `user-preferences`: personal, page-scoped preferences with independent editors and permission policies.

### Modified Capabilities

## Impact

- Persistence schema and versioned migration for user-owned preference data.
- Authenticated server API with per-user and per-page authorization.
- Next.js page-specific editors and reusable Playwright coverage.
- Persistence architecture: `docs/architecture/persistence.md`.
- Personal and page-specific authorization: `docs/product/roles-and-permissions.md`.
- Reusable browser E2E coverage: `docs/development/testing.md`.

No new dependency or shared/global preference system.
