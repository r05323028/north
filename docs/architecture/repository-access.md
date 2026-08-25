# Repository access

## Model

- Server stores metadata only: `id`, `name`, `url`, `description`
  (Admin/Owner manage these in Repository Settings).
- The daemon clones/reads using the **host's normal Git environment**: system
  `git`, SSH config/agent, credential helpers, authenticated `gh`, file
  permissions. If `git clone <url>` works in the daemon host shell, North works.
- Inspections record the resolved commit SHA so assessments cite exact source.

## Read-only guarantee — stated honestly

The invariant is:

> Requirement clarification must never intentionally persist mutations to
> configured source repositories.

A Git **command allowlist alone does NOT provide this guarantee** — a
coding-capable runtime can modify working-tree files directly without invoking
Git. North 0.1.0 therefore states its actual enforcement level and uses the
smallest credible mechanism:

- Inspection tasks run against a **disposable checkout** managed by the daemon;
- the daemon treats any **dirty working tree** detected after a clarification
  task as an invariant violation: the workspace is discarded and the incident
  reported;
- mutation detection is process-level, NOT kernel/sandbox-enforced. North does
  not claim OS-level read-only isolation in 0.1.0.

If stronger isolation becomes necessary (read-only mounts, sandboxed runtimes),
it can replace the disposable-checkout mechanism without changing product
semantics.

## Never (unchanged)

- No centralized credential manager; no SSH keys/PATs sent to the server.
- Agent does not own repository credentials; server receives no Git credentials;
  daemon uses the host's existing git/auth environment.

## Workspaces

The daemon owns local workspace management. Prefer the simplest safe approach;
do not introduce git worktrees until coding/modification features require them.
The design leaves room to adopt worktree isolation later without protocol breaks.

Out of scope for 0.1.0: push, PR creation, branch selection UI, arbitrary sync.
