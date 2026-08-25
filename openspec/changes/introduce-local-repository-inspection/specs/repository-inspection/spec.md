## Purpose

Lets the daemon read configured repositories using nothing but the host's
existing Git setup, strictly read-only, citing exact commits.

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

### Requirement: Strictly read-only inspection

The daemon SHALL issue only read-class git operations (clone/fetch/rev-parse/
log/diff-class reads) against configured repositories. It SHALL NOT push,
commit, mutate refs, or modify working trees of configured sources.

#### Scenario: Mutation attempt is impossible

- **WHEN** the command allowlist is consulted for any write operation
- **THEN** no path exists for push/commit against inspected repositories

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
