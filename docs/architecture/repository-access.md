# Repository access

## Configured repository model

The server stores metadata only:

- `id`: immutable durable UUID identity;
- `name`: trimmed editable display name, at most 100 UTF-8 bytes;
- `name_normalized`: persistence-only non-locale lowercase key, unique across
  enabled and disabled rows;
- `url`: trimmed supported Git location, immutable after creation in 0.1.0;
- `description`: trimmed editable text, empty allowed, at most 10,000 UTF-8
  bytes;
- `created_at`, `updated_at`, and nullable `disabled_at` server timestamps.

Normal Remove always means soft-disable. It sets `disabled_at` even when no
readiness evidence references the row; repeating Remove is idempotent and does
not delete the row. Admin/Owner can re-enable the same identity, which clears
`disabled_at`. North 0.1.0 has no normal hard-delete repository operation.

Repository names are unique after trimming and non-locale Unicode lowercase.
Create conflicts with both enabled and disabled names; a disabled-name conflict
points to re-enable of the retained row rather than creating a duplicate. Both
management and active reads sort by `name_normalized ASC, id ASC`.

## Authorization and reads

Admin and Owner can create, edit name/description, disable, re-enable, and read
the management list. Requester and Requirement Manager are refused server-side.
This management permission is not implied by a session or inspection consuming
catalog metadata.

Management list includes enabled and disabled rows, status, and current metadata
for Settings lifecycle controls. Active runtime catalog contains only rows with
`disabled_at IS NULL`; it supplies enabled metadata for new session context and
inspection candidates. Disabled rows remain visible to authorized management
and historical reads even though active selection excludes them.

## URL validation and credentials

Server validation trims and bounds URLs to 2,048 UTF-8 bytes. Supported 0.1
shapes are:

```text
https://<host>/<non-empty path>
ssh://[git@]<host>/<non-empty path>
git@<host>:<non-empty path>
```

HTTPS userinfo, every URL password, and SSH/SCP users other than literal `git`
are rejected. These examples are invalid:

```text
https://user:password@example.com/repo.git
https://token@example.com/org/repo.git
```

Normal SSH identity syntax remains valid:

```text
git@github.com:org/repo.git
ssh://git@github.com/org/repo.git
```

North stores only the validated location string. No SSH keys, PATs, tokens,
passwords, or credential-helper contents enter server persistence or protocol
DTOs. The daemon uses the host's normal Git environment: system `git`, SSH
config/agent, credential helpers, authenticated `gh`, and file permissions.
URL shape validation does not test access; inspection owns host-Git errors.

## Historical identity

Readiness evidence retains `repository_id` and the full resolved commit SHA.
Disabling retains the row and current name/description/URL, so historical joins
remain human-readable without active-catalog membership. Name or description
changes affect current display only; they never change repository ID, immutable
URL, or recorded SHA. Replacing a source means disable the old row and create a
new identity.

## Workspace boundary

The configured-repositories change owns catalog metadata and lifecycle only.
`introduce-local-repository-inspection` consumes enabled metadata and owns host
Git access, reusable cache/fetch/clone behavior, unique disposable checkouts,
concurrency isolation, dirty-tree detection, disposal, and full commit-SHA
resolution/reporting.

```text
daemon repository cache (per repository, never runtime working tree)
        ├── disposable checkout: session A / task A
        └── disposable checkout: session B / task B
```

The cache is reusable source material. Each clarification execution receives a
unique mutable checkout scoped by session/task and repository ID. A plain local
copy/clone-from-cache is sufficient; North 0.1.0 does not require Git
worktrees. Concurrent sessions inspecting one repository never share a mutable
directory, and runtime changes cannot contaminate the cache or another session.

After every task, the daemon checks the checkout for unexpected dirty changes.
A dirty result is an invariant violation: report it and discard the checkout
before reuse. This is process-level protection, not kernel or sandbox
isolation. North does not claim OS-level read-only enforcement in 0.1.0.

Out of scope for configured-repositories: clone/fetch execution, push, PR
creation, branch-selection UI, arbitrary sync, source inspection, and
intentional source-repository mutation.
