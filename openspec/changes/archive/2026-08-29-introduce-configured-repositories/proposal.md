# Introduce repository configuration

## Why

Agents need to know which source repositories are relevant. The server owns the
catalog as credential-free metadata, and historical readiness evidence must
remain readable after a repository is removed from active use.

## Scope owned by this change

This change owns only the configured repository catalog:

- durable repository identity, metadata, existence, and lifecycle model;
- migration, constraints, persistence, and server-side validation;
- Admin/Owner management operations and authorization;
- enabled runtime catalog and separate management list;
- soft-disable/re-enable lifecycle and history-preserving identity;
- the durable repository identity contract consumed by readiness evidence; this
  change owns row existence, while readiness owns evidence acceptability and
  promotion decisions;
- credential-free server boundary;
- Repository Settings UI and its frontend validation;
- Rust, web, and OpenSpec validation gates.

It does not own Git access or inspection execution.

## What Changes

- Store immutable UUID `id`, editable `name` and `description`, immutable
  0.1.0 `url`, and server timestamps with explicit create, metadata-change,
  disable, re-enable, and idempotent no-op semantics. A derived normalized-name
  key enforces case-insensitive uniqueness.
- Trim and bound all metadata on the server. Accept only supported Git URL
  shapes and reject embedded URL credentials. North 0.1 deliberately permits
  only the standard literal `git` SSH/SCP transport username; normal
  `git@host:path` and `ssh://git@host/path` syntax remains valid.
- Admins and Owners can create, update metadata, disable, re-enable, and view
  the complete management list. Requesters and Requirement Managers are
  rejected server-side.
- Normal Remove always soft-disables, even when no readiness evidence references
  the row. North 0.1 exposes no hard-delete management operation.
- Re-enable clears `disabled_at` on the same durable row. A disabled normalized
  name cannot create a second row; the conflict directs the caller to re-enable
  the existing identity.
- Management list includes enabled and disabled rows. The active runtime
  catalog is an internal server/persistence read used for server-assembled
  `session.start` context and downstream inspection orchestration; it includes
  only rows with `disabled_at IS NULL`. It is not a browser management endpoint
  or a daemon endpoint for independently fetching the catalog. Both reads use
  deterministic ordering.
- Historical evidence is authoritative as `repository_id` plus full
  `commit_sha`; the retained row supplies current human-readable metadata. URL
  immutability prevents one durable identity from silently changing to an
  unrelated source. Name/description are not historical snapshots, and disable
  or re-enable never changes prior evidence or identity.
- Settings UI shows enabled/disabled status and re-enable, never hard-delete.

## Explicitly out of scope

The following belong to `introduce-local-repository-inspection` and are not
implementation tasks here:

- host Git invocation, credential use, and Git URL access;
- reusable Git caches or clone/fetch/sync behavior;
- disposable checkouts, worktrees, concurrency isolation, or dirty-tree
  detection;
- commit resolution/reporting and source inspection.

This change may publish enabled metadata for those downstream consumers, but it
does not implement their runtime.

## Capabilities

### New Capabilities

- `repositories`: credential-free metadata catalog with Admin/Owner management,
  deterministic enabled catalog, soft disable, re-enable, and history
  preservation.

### Modified Capabilities

(none)

## Impact

- Affected docs: `docs/architecture/repository-access.md` and
  `docs/architecture/persistence.md`.
- Canonical prerequisites: existing role permissions, readiness evidence, and
  repository-isolation contracts. They are established contracts, not pending
  implementation work in this change. Readiness/server persistence must reject
  unknown cited repository IDs and enforce session/run provenance; this change
  supplies the durable identity and lifecycle contract without moving readiness
  acceptance into repository CRUD.
- Downstream consumer: `introduce-local-repository-inspection` consumes enabled
  metadata and owns host Git/cache/checkout/SHA behavior.
- The protocol change may consume the enabled catalog in server-assembled
  `session.start` context; this change does not own protocol delivery.
- No SSH keys, PATs, tokens, passwords, credential-helper contents, or other
  secret material is accepted or persisted by North.

## Validation gate

Implementation is not complete until repository persistence/API tests,
permission and lifecycle tests, URL credential tests, Settings UI tests, the
Rust gate, frontend lint/typecheck/build, and `openspec validate --all --strict`
pass. The final implementation must also prove that readiness citations resolve
to existing durable repository identities, that disable-during-inflight evidence
remains valid without re-enabling the row, that timestamp no-op semantics hold,
that the chosen SSH username policy is covered by tests, that no hard-delete
path exists, and that history remains interpretable after disable.
