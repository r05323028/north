# Introduce local repository inspection runtime

## Why

Assessments must cite real source. The daemon reads configured repositories
with the host's own Git environment — exactly what works in its shell — and
records what it saw.

## What Changes

- Daemon workspace management: clone-on-demand / fetch-existing using host
  `git`, SSH agent, and credential helpers; nothing custom.
- Read-class Git commands plus disposable-checkout enforcement: any dirty tree after clarification is a process-level invariant violation; default branch reading. The mechanism is not a kernel sandbox.
- Inspections report repository id + resolved commit SHA back through events.

No code modification, no push, no PR creation, no centralized credentials —
structurally excluded, not just discouraged.

## Capabilities

### New Capabilities

- `repository-inspection`: workspace lifecycle, host-git usage, read-only
  guarantee, commit-SHA reporting.

### Modified Capabilities

- `readiness`: repositories_reviewed entries gain real SHAs from inspections.

## Impact

- Affected docs: docs/architecture/repository-access.md (canonical),
  docs/architecture/daemon.md (workspace section).
- Dependencies on earlier changes: introduce-configured-repositories,
  introduce-server-daemon-protocol.
