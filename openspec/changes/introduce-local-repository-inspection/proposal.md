# Introduce local repository inspection runtime

## Why

Assessments must cite real source. The daemon reads configured repositories
with the host's own Git environment — exactly what works in its shell — and
records what it saw without allowing concurrent clarification sessions to share
mutable working trees.

## What Changes

- Daemon keeps a reusable per-repository cache/fetch source under its config
  directory, but every session/task receives a unique disposable checkout.
- Host `git`, SSH agent, and credential helpers remain the access mechanism;
  no server-side credentials or custom auth.
- Read-class Git commands plus post-task dirty-tree detection: any mutation in
  a disposable checkout is a process-level invariant violation; discard and
  report it. This is not kernel sandboxing.
- Inspections report repository id + full resolved commit SHA through typed
  events; disabled/unknown repositories cannot start new inspections.

No code modification, push, PR creation, centralized credentials, Git
worktrees, or intentional source-repository mutation.

## Capabilities

### New Capabilities

- `repository-inspection`: host-Git access, cache/checkouts, concurrency-safe
  workspace lifecycle, read-only guarantee, and commit-SHA reporting.

### Modified Capabilities

- `readiness`: repositories_reviewed entries gain real SHAs from inspections.

## Impact

- Affected docs: docs/architecture/repository-access.md (canonical),
  docs/architecture/daemon.md (workspace section).
- Cross-cutting contract: `harden-distributed-system-architecture`.
- Dependencies on earlier changes: introduce-configured-repositories,
  introduce-server-daemon-protocol.
