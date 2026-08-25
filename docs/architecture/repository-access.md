# Repository access

## Model

- Server stores metadata only: `id`, `name`, `url`, `description`, timestamps,
  and nullable `disabled_at` (Admin/Owner manage these in Repository Settings).
- Normal Remove means **soft-disable**: set `disabled_at`; keep the row and
  identity available to historical assessments. Disabled repositories are
  excluded from normal catalog and new-inspection selection.
- The daemon clones/reads using the **host's normal Git environment**: system
  `git`, SSH config/agent, credential helpers, authenticated `gh`, and file
  permissions. If `git clone <url>` works in the daemon host shell, North works.
- Inspections record repository id and the full resolved commit SHA so
  assessments cite exact source.

## Credentials stay local

- No centralized credential manager; no SSH keys, PATs, tokens, or passwords
  sent to or stored by the server.
- Daemon uses host Git/auth environment. Repository configuration schemas must
  contain metadata and lifecycle fields only.

## Workspace model

```text
daemon repository cache (per repository, never runtime working tree)
        ├── disposable checkout: session A / task A
        └── disposable checkout: session B / task B
```

The cache is reusable source material. Each clarification execution receives a
unique mutable checkout scoped by session/task and repository id. A plain local
copy/clone-from-cache is sufficient; North 0.1.0 does not require Git
worktrees. Concurrent sessions inspecting one repository never share a mutable
directory, and runtime changes cannot contaminate the cache or another session.

After every task, the daemon checks the checkout for unexpected dirty changes.
A dirty result is an invariant violation: report it and discard the checkout
before reuse. Clean checkouts are disposable too. This is process-level
protection, NOT kernel or sandbox isolation; North does not claim OS-level
read-only enforcement in 0.1.0.

## Source and history

Inspection must resolve the checked-out commit and report the full SHA in its
assessment evidence. Unknown or disabled repositories fail before a new
inspection starts. Historical rows remain readable after a repository is
removed from the active catalog because normal removal does not delete them.

Out of scope for 0.1.0: push, PR creation, branch-selection UI, arbitrary sync,
and intentional source-repository mutation.
