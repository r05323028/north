# Tasks

## 1. Domain model and schema

- [ ] 1.1 Add migration 0006 for `repositories` with immutable UUID `id`, trimmed `name`, persistence-only `name_normalized`, immutable `url`, editable `description`, `created_at`, `updated_at`, nullable `disabled_at`, and no credential columns.
- [ ] 1.2 Add repository domain/value validation for name, description, supported Git URL shape, URL immutability, and derived normalized-name key.
- [ ] 1.3 Add database uniqueness and indexes for `name_normalized` across enabled and disabled rows, deterministic `name_normalized ASC, id ASC` reads, and historical row retention.
- [ ] 1.4 Add persistence mappings and transactional create/update/disable/re-enable operations with server-generated timestamps and no hard-delete method.

## 2. Server validation and credential boundary

- [ ] 2.1 Enforce server-side trimming and limits: name non-empty/≤100 UTF-8 bytes, description empty/≤10,000 bytes, URL non-empty/≤2,048 bytes.
- [ ] 2.2 Implement normalized-name conflict handling using non-locale Unicode lowercase, including concurrent enabled and disabled-name conflicts.
- [ ] 2.3 Implement supported Git URL parsing for HTTPS, `ssh://[git@]`, and `git@host:path`; reject malformed/empty host/path and unsupported schemes.
- [ ] 2.4 Reject HTTPS userinfo, every URL password, and SSH/SCP users other than literal `git`; add tests preserving normal `git@` SSH syntax.
- [ ] 2.5 Reject attempted URL mutation with an immutable-field conflict and prove no metadata/history mutation occurs.

## 3. Management API and authorization

- [ ] 3.1 Add Admin/Owner-only create endpoint with normalized validation and explicit conflict response.
- [ ] 3.2 Add Admin/Owner-only metadata update for name/description; keep URL immutable on enabled and disabled rows.
- [ ] 3.3 Add Admin/Owner-only normal Remove that always soft-disables, including unreferenced rows; make repeated Remove idempotent.
- [ ] 3.4 Add Admin/Owner-only re-enable that clears `disabled_at` on the same UUID and never inserts a duplicate.
- [ ] 3.5 Add Admin/Owner management list containing enabled and disabled rows, status, metadata, and deterministic ordering.
- [ ] 3.6 Reject Requester and Requirement Manager server-side for every management mutation and management-list read; add no hard-delete route or command.

## 4. Catalog and history behavior

- [ ] 4.1 Add separate enabled-only active catalog read filtered by `disabled_at IS NULL`, with deterministic ordering for session/inspection consumers.
- [ ] 4.2 Preserve `repository_id` plus full `commit_sha` history and retained current metadata after disable; add disable/history/re-enable integration tests.
- [ ] 4.3 Add disabled-name create conflict identifying the retained row and directing re-enable; prove no duplicate durable identity.
- [ ] 4.4 Verify catalog lifecycle never starts Git clone, fetch, checkout, dirty-tree detection, or source inspection work owned by `introduce-local-repository-inspection`.

## 5. Settings UI

- [ ] 5.1 Add Admin/Owner Repository Settings table showing enabled and disabled rows with `disabled_at` status.
- [ ] 5.2 Add create and name/description edit forms with frontend validation and server error/conflict display; URL is read-only after creation.
- [ ] 5.3 Add Remove-as-disable and re-enable controls; provide no hard-delete affordance and preserve disabled rows in management view.
- [ ] 5.4 Add frontend tests/checks for status visibility, disabled-name conflict, URL immutability, and permission-safe behavior.

## 6. Validation gates

- [ ] 6.1 Add Rust unit/integration/PostgreSQL tests for schema, normalization, URL credential rejection, permissions, conflicts, ordering, lifecycle, and history.
- [ ] 6.2 Run `cd apps/web && npm run lint`, `npm run typecheck`, and `npm run build` after Settings UI implementation.
- [ ] 6.3 Run `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace`.
- [ ] 6.4 Run `openspec validate --all --strict` and `git diff --check`; do not mark repository implementation complete while any required catalog or validation task is unchecked.
