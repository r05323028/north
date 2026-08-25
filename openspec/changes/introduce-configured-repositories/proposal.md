# Introduce repository configuration

## Why

Agents need to know which source repositories are relevant. The server owns the
catalog as credential-free metadata, and historical readiness evidence must
remain readable after a repository is removed from active use.

## What Changes

- Admin/Owner CRUD for repositories: id, name, URL, description, timestamps,
  and nullable `disabled_at`.
- Normal Remove soft-disables a repository instead of deleting its durable
  identity row. Active catalog/inspection selection excludes disabled rows;
  historical assessments retain repository id and exact commit SHA.
- Repository list is surfaced in settings UI; enabled catalog is available to
  daemon/session context when the protocol change lands.
- Schema forbids credential fields entirely; credentials remain in daemon host
  Git configuration.

Out of scope: centralized Git credential management, storing SSH keys/PATs,
branch selection, sync scheduling, and normal hard deletion of referenced
repositories.

## Capabilities

### New Capabilities

- `repositories`: metadata catalog with admin-gated management, soft disable,
  and history preservation.

### Modified Capabilities

(none)

## Impact

- Affected docs: docs/architecture/repository-access.md and
  docs/architecture/persistence.md.
- Cross-cutting contract: `harden-distributed-system-architecture`.
- Dependencies on earlier changes: introduce-role-and-permission-model.
