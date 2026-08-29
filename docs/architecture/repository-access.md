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

Normal Remove always means soft-disable. It sets `disabled_at` to the server's
current UTC time and advances `updated_at` only when an enabled row changes.
Repeating Remove on an already disabled row is a true idempotent no-op: both
`disabled_at` and `updated_at` remain unchanged. Re-enable clears `disabled_at`
on the same identity and advances `updated_at`; re-enable of an already-enabled
row is a true no-op with `updated_at` unchanged. Create sets `created_at` and
`updated_at` to now with `disabled_at = null`; metadata changes advance
`updated_at`; `created_at` never changes. North 0.1.0 has no normal hard-delete
repository operation.

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
`disabled_at IS NULL`; it is an internal server/persistence read used for
server-assembled `session.start` context and downstream inspection candidates.
The daemon receives relevant enabled metadata through `session.start` and does
not independently fetch a repository catalog. North 0.1.0 creates no public
browser catalog-management or standalone daemon-catalog endpoint merely because
this internal read is called a catalog. Disabled rows remain visible to
authorized management and historical reads even though active selection
excludes them.

## URL validation and credentials

Server validation trims and bounds URLs to 2,048 UTF-8 bytes. Supported 0.1
shapes are:

```text
https://<host>/<non-empty path>
ssh://[git@]<host>/<non-empty path>
git@<host>:<non-empty path>
```

HTTPS userinfo, every URL password, and SSH/SCP users other than literal `git`
are rejected. North 0.1 deliberately supports only the standard literal `git`
SSH/SCP transport username, even if a host could clone a URL using another
username; this is an explicit 0.1 product URL policy, not a claim that every
host-Git-valid username is a secret. These examples are invalid:

```text
https://user:password@example.com/repo.git
https://token@example.com/org/repo.git
ssh://git:password@git.internal/repo.git
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
The repository-inspection integration fixture uses local bare Git repositories to
prove cache, revision, isolation, and cleanup behavior. Its Unix tests use a
temporary Git home, local HTTP challenge, and fake `core.sshCommand` to prove
credential-helper and SSH-config handoff without operator credentials; they do
not simulate an operator's SSH agent. Production Git processes inherit those host
access mechanisms while removing Git config/path-override settings that could
redirect a prepared checkout and restoring only host authentication settings. Server-assembled repository
contexts remain the only production source of repository locations; local paths
in tests exercise the same host-Git process without widening server URL policy.

## Historical identity

Readiness evidence retains `repository_id` and the full resolved commit SHA.
The cited identity MUST resolve to a retained durable configured-repository row;
unknown IDs are rejected before accepted evidence. Disabling retains the row and
current name/description/URL, so historical joins remain human-readable without
active-catalog membership. URL is immutable in 0.1 and keeps source identity
stable. Name or description changes affect current display only; they do not
create historical metadata snapshots and never change repository ID, immutable
URL, or recorded SHA. Disabling does not alter evidence; re-enable returns the
same identity. An in-flight citation may remain valid after disable when it was
included in the session context or explicitly inspected under that authorized
run; new inspection selection still requires enabled state.

## Readiness citations and disable races

`introduce-configured-repositories` owns durable repository identity, row
existence, and lifecycle. Readiness owns whether evidence is acceptable for a
Requirement. `introduce-local-repository-inspection` owns source inspection and
exact commit-SHA production. `north-protocol` carries `repository_id` and
`commit_sha` as typed facts and validates complete Git SHA-1/SHA-256 object IDs; it
never accesses repository persistence.

A new inspection may select only an enabled row. If `session.start` supplied R
while enabled, inspection began, and an Admin then disabled R, a later
assessment remains eligible to cite R and its exact SHA: the retained row must
exist and the citation must be valid for that session/run. Disable prevents
future selection; it does not invalidate legitimate in-flight historical
evidence solely because lifecycle state changed.
No new provenance subsystem is introduced; existing session context and
inspection-result contracts provide the run binding.

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
worktrees. Cache and disposable roots are daemon-owned mode-0700 namespaces;
path checks are process-level protection, not a kernel sandbox. Concurrent sessions inspecting one repository never share a mutable
directory, and runtime changes cannot contaminate the cache or another session.

After every task, the daemon checks the checkout for unexpected dirty changes.
A dirty result is an invariant violation: report it and discard the checkout
before reuse. This is process-level protection, not kernel or sandbox
isolation. North does not claim OS-level read-only enforcement in 0.1.0.

Out of scope for configured-repositories: clone/fetch execution, push, PR
creation, branch-selection UI, arbitrary sync, source inspection, and
intentional source-repository mutation.
