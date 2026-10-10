# Design

## Context

See `proposal.md` for motivation and `specs/user-preferences/spec.md` for behavior. The Requirement workspace currently keeps view, search, status, creator, and sort controls in browser component state. `/auth/me` is a read-only identity endpoint; repository and daemon settings are shared, administrator-gated configuration and are outside this feature.

## Goals / Non-Goals

**Goals:** Persist a small, explicit set of Requirement-workspace display defaults per signed-in user; enforce ownership in the server; keep page-specific editors and authorization decisions separate; prove save/reload behavior with reusable tests.

**Non-Goals:** A generic preference registry or arbitrary JSON store; AI, notification, search-history, or shared-instance preferences; preferences on admin/settings/detail pages; changing Requirement permissions or `/auth/me`.

## Decisions

1. **Use explicit typed columns for v1.** Add one `user_requirement_preferences` row per user, keyed by `user_id`, with columns for default view, optional List status, optional creator, List sort, and `updated_at`. Constrain enum-like values in the database and validate them in the server DTO. This matches the four known controls and avoids a speculative JSONB/key-value framework. A future page adds its own explicit fields and policy only when requested. A separate JSONB preferences table was considered but rejected: it would accept an open-ended shape without solving a current need.

2. **Derive ownership from the session.** Expose current-user GET and full-replacement PUT operations under `/auth/me/preferences/requirement-workspace`; accept no user ID from clients. The server uses the authenticated `CurrentUser` and persistence upsert. This avoids IDOR-prone `/users/{id}` writes and leaves `/auth/me` read-only. All authenticated roles may edit their own Requirement-workspace values; no role, including Admin or Owner, can edit another user's values. Future page endpoints must state their own page authorization explicitly rather than inheriting a global preference role matrix.

3. **Save defaults explicitly.** Keep current filtering/search behavior immediate and transient. Add a page-specific “Save as default” action that stores the selected board/list view and List status, creator, and sort. Search text is excluded. Missing rows return defaults (board, all statuses, all creators, `updated`). Invalid or unknown input fails before persistence; each successful PUT replaces the complete validated preference object atomically.

4. **Separate server and browser proofs.** PostgreSQL-backed integration tests cover migration, defaults, replacement, validation, and cross-user isolation. The assembled Playwright release suite covers sign-in, save, reload, and restored workspace controls. Use one small page-editor test helper with page-specific locators/actions so later editors can reuse the flow without sharing their UI or permission rules.

Alternatives: autosaving every control change risks persisting transient query state; a shared settings page would confuse personal display defaults with instance configuration; a generic cross-page permission matrix would couple unrelated pages.

## Risks / Trade-offs

- **A future page needs new typed fields and authorization code** → add them only with a concrete page requirement; do not generalize before a second use case exists.
- **Concurrent tabs can overwrite defaults** → accept last-successful-save-wins for low-impact display settings; do not add revision/conflict machinery.
- **A database constraint and server validator can drift** → share canonical enum values where practical and test invalid values at both HTTP and persistence boundaries.
- **Additive migration rollback** → retain the new table and migration; rollback application code without deleting user preferences.

## Migration Plan

Add an append-only SeaORM migration and entity for `user_requirement_preferences`; no backfill is needed because no preferences exist. Exercise the migration on an empty database and the existing test database path. Do not change the repository's migration startup/CLI policy. Rollback never drops the table or deletes saved values.
