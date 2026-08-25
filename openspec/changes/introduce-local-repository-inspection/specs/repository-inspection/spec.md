## Purpose

Lets the daemon read configured repositories with the host's existing Git setup,
while detecting and discarding clarification-task mutations and citing exact commits.

## ADDED Requirements

### Requirement: Host-environment Git access

Repository access SHALL use the host's normal `git` binary and environment
(SSH config/agent, credential helpers). If cloning the URL works from the
host shell, inspection SHALL succeed without extra configuration; if not, it
SHALL fail with the underlying git error surfaced.

#### Scenario: Shell-equivalent access

- **WHEN** `git clone <url>` succeeds in the daemon host shell
- **THEN** the daemon prepares a readable workspace for that URL with no
additional credentials provided by North

### Requirement: No persisted mutations to source repositories

The invariant: requirement clarification must never intentionally persist
mutations to configured source repositories. The Git command allowlist alone
does NOT provide this guarantee (a runtime can modify working-tree files
directly), so 0.1.0 enforcement is process-level: inspection tasks run in a
daemon-managed DISPOSABLE checkout, and any dirty working tree detected after
a clarification task SHALL be treated as an invariant violation — the
workspace is discarded and the incident reported. This is NOT kernel/sandbox-
enforced, and the documentation SHALL NOT claim OS-level read-only isolation.
Git operations themselves stay read-class only (no push/commit/ref mutation).

#### Scenario: Dirty tree after clarification is a violation

- **WHEN** the daemon detects an unexpected working-tree change after a task
- **THEN** the workspace is discarded and the violation recorded, so no
mutation reaches the configured source checkout

### Requirement: Inspections cite exact commits

Every inspection SHALL resolve and report the checked-out commit SHA together
with the repository identity, making assessments reproducible against exact
source states.

#### Scenario: Assessment can name its basis

- **WHEN** the agent inspects a repository during clarification
- **THEN** the resulting event carries repository_id + full commit SHA

### Requirement: Workspaces are daemon-managed and boring

The daemon SHALL manage local workspace storage (clone once, fetch to refresh)
without introducing worktree isolation in 0.1.0, leaving room to adopt it
later without protocol breaks.

#### Scenario: Repeat inspection reuses the clone

- **WHEN** the same repository is inspected twice
- **THEN** the second run fetches/reuses rather than recloning from scratch
