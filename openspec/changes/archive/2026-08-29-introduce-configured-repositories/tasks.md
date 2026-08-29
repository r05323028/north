# Tasks

## 1. Domain model and schema

- [x] 1.1 Add migration 0006 for `repositories` with immutable UUID `id`, trimmed `name`, persistence-only `name_normalized`, immutable `url`, editable `description`, `created_at`, `updated_at`, nullable `disabled_at`, and no credential columns; encode the timestamp/no-op lifecycle contract.
- [x] 1.2 Add repository domain/value validation for name, description, supported Git URL shape, URL immutability, and derived normalized-name key.
- [x] 1.3 Add database uniqueness and indexes for `name_normalized` across enabled and disabled rows, deterministic `name_normalized ASC, id ASC` reads, and historical row retention.
- [x] 1.4 Add persistence mappings and transactional create/update/disable/re-enable operations with server-generated timestamps, exact idempotent no-op behavior, and no hard-delete method.

## 2. Server validation and credential boundary

- [x] 2.1 Enforce server-side trimming and limits: name non-empty/≤100 UTF-8 bytes, description empty/≤10,000 bytes, URL non-empty/≤2,048 bytes.
- [x] 2.2 Implement normalized-name conflict handling using non-locale Unicode lowercase, including concurrent enabled and disabled-name conflicts.
- [x] 2.3 Implement supported Git URL parsing for HTTPS, `ssh://[git@]`, and `git@host:path`; reject malformed/empty host/path and unsupported schemes.
- [x] 2.4 Reject HTTPS userinfo, every URL password, and SSH/SCP users other than literal `git`; document that North 0.1 intentionally chooses the standard literal-`git` username policy and add valid/invalid URL tests, including rejection of `deploy@`/other non-`git` usernames.
- [x] 2.5 Reject attempted URL mutation with an immutable-field conflict and prove no metadata/history mutation occurs.

## 3. Management API and authorization

- [x] 3.1 Add Admin/Owner-only create endpoint with normalized validation and explicit conflict response.
- [x] 3.2 Add Admin/Owner-only metadata update for name/description; keep URL immutable on enabled and disabled rows.
- [x] 3.3 Add Admin/Owner-only normal Remove that always soft-disables, including unreferenced rows; make repeated Remove idempotent.
- [x] 3.4 Add Admin/Owner-only re-enable that clears `disabled_at` on the same UUID and never inserts a duplicate.
- [x] 3.5 Add Admin/Owner management list containing enabled and disabled rows, status, metadata, and deterministic ordering.
- [x] 3.6 Reject Requester and Requirement Manager server-side for every management mutation and management-list read; add no hard-delete route or command.

## 4. Catalog and history behavior

- [x] 4.1 Add separate enabled-only active catalog read filtered by `disabled_at IS NULL`, with deterministic ordering for session/inspection consumers; keep it an internal server/persistence read for server-assembled `session.start` context and downstream inspection orchestration, not a new browser or daemon catalog endpoint.
- [x] 4.2 Preserve `repository_id` plus full `commit_sha` history and retained current metadata after disable; add disable/history/re-enable integration tests, including immutable URL and no metadata-snapshot claims.
- [x] 4.3 Add disabled-name create conflict identifying the retained row and directing re-enable; prove no duplicate durable identity.
- [x] 4.4 Verify catalog lifecycle never starts Git clone, fetch, checkout, dirty-tree detection, or source inspection work owned by `introduce-local-repository-inspection`.
- [x] 4.5 Define the cross-change readiness contract: server readiness/persistence rejects unknown cited `repository_id` before evidence or promotion, while configured repositories own durable row existence/lifecycle and `north-protocol` performs only non-empty structural validation of `repository_id`/`commit_sha`.
- [x] 4.6 Test disable-during-inflight behavior: new inspection selection requires enabled state, while an already-running session's valid citation remains eligible after disable when the retained row and session/run provenance remain valid; disable alone must not reject the evidence, and no new provenance subsystem is added.
- [x] 4.7 Test exact lifecycle timestamp semantics for create, metadata change, disable, repeated disable, re-enable, and repeated re-enable.

## 5. Settings UI

- [x] 5.1 Add Admin/Owner Repository Settings table showing enabled and disabled rows with `disabled_at` status.
- [x] 5.2 Add create and name/description edit forms with frontend validation and server error/conflict display; URL is read-only after creation.
- [x] 5.3 Add Remove-as-disable and re-enable controls; provide no hard-delete affordance and preserve disabled rows in management view.
- [x] 5.4 Add frontend tests/checks for status visibility, disabled-name conflict, URL immutability, and permission-safe behavior.

## 6. Validation gates

- [x] 6.1 Add Rust unit/integration/PostgreSQL tests for schema, normalization, URL credential rejection, the chosen literal-`git` username policy, permissions, conflicts, ordering, lifecycle timestamps/no-ops, citation existence/provenance, disable-during-inflight behavior, and history.
- [x] 6.2 Run `cd apps/web && npm run lint`, `npm run typecheck`, and `npm run build` after Settings UI implementation.
- [x] 6.3 Run `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace`.
- [x] 6.4 Run `openspec validate --all --strict` and `git diff --check`; do not mark repository implementation complete while any required catalog or validation task is unchecked.
