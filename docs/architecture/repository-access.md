# Repository access

0.1.0 inspection is **read-only**.

## Model

- Server stores metadata only: `id`, `name`, `url`, `description`
  (Admin/Owner manage these in Repository Settings).
- The daemon clones/reads using the **host's normal Git environment**: system
  `git`, SSH config/agent, credential helpers, authenticated `gh`, file
  permissions. If `git clone <url>` works in the daemon host shell, North works.
- Inspections record the resolved commit SHA so assessments cite exact source.

## Never

- No centralized credential manager; no SSH keys/PATs sent to the server.
- No mutation of configured source repositories; analysis is read-only.
  (Requirement clarification must not modify source.)
- Agent does not own repository credentials; server needs none for daemon git work.

## Workspaces

The daemon owns local workspace management. Prefer the simplest safe approach;
do not introduce git worktrees until coding/modification features require them.
The design leaves room to adopt worktree isolation later without protocol breaks.

Out of scope for 0.1.0: push, PR creation, branch selection UI, arbitrary sync.
