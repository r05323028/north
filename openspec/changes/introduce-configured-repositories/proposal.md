# Introduce repository configuration

## Why

Agents need to know which source repositories are relevant. The server owns
the catalog as pure metadata; credentials never enter North.

## What Changes

- Admin/Owner CRUD for repositories: id, name, url, description.
- Repository list surfaced in settings UI; catalog made available to the
  daemon path when the protocol change lands.
- Schema forbids credential fields entirely.

Out of scope: centralized Git credential management, storing SSH keys/PATs,
branch selection, sync scheduling.

## Capabilities

### New Capabilities

- `repositories`: metadata catalog with admin-gated management.

### Modified Capabilities

(none)

## Impact

- Affected docs: docs/architecture/repository-access.md (catalog section).
- Dependencies on earlier changes: introduce-role-and-permission-model.
