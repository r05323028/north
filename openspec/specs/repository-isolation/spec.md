# repository-isolation Specification

## Purpose

Provides isolated, disposable clarification workspaces and durable repository identity so concurrent inspections remain safe and historical evidence remains understandable without centralizing Git credentials.

## Requirements

### Requirement: Clarification workspaces are session-isolated

A reusable daemon repository cache SHALL never be a runtime working tree. Each
clarification execution SHALL receive a unique disposable checkout scoped to
session/task and repository identity. Two concurrent sessions inspecting the
same repository SHALL use different mutable directories. Runtime file changes
MUST remain inside that disposable checkout; a contaminated checkout SHALL be
discarded and reported. North 0.1.0 SHALL describe this as process-level
protection, not kernel or sandbox isolation, and SHALL not require Git
worktrees.

#### Scenario: Concurrent inspections do not share files

- **WHEN** sessions A and B inspect repository X concurrently
- **THEN** each runtime sees its own disposable checkout and a mutation in A cannot appear in B or the reusable cache

#### Scenario: Dirty checkout is discarded

- **WHEN** a clarification task ends with an unexpected dirty working tree
- **THEN** the daemon reports an invariant violation and discards that checkout before another task can use it

### Requirement: Repository identity is soft-disabled and credential-free

Configured repositories SHALL retain their durable row and identity after a
normal Remove operation by setting `disabled_at`; hard deletion of a referenced
repository SHALL not be the normal 0.1.0 path. New inspections SHALL exclude
disabled repositories. Readiness evidence SHALL retain the repository id and
exact commit SHA, and the retained metadata row SHALL keep that evidence
human-readable. Server-side repository configuration SHALL contain no Git
credentials, tokens, keys, or passwords; credentials remain in the daemon
host's Git environment.

#### Scenario: Remove preserves assessment history

- **WHEN** an Admin removes repository X after an assessment recorded X at commit `abc123`
- **THEN** X is disabled rather than deleted, the old assessment still resolves to X and `abc123`, and new inspections cannot select X

#### Scenario: Repository credentials stay local

- **WHEN** the server persists configured repository metadata
- **THEN** its schema contains identity/URL/description and lifecycle metadata only, with no credential material

### Requirement: Inspections cite exact source revisions

Every successful inspection SHALL report the configured repository identity and
full resolved commit SHA from its disposable checkout. A disabled or unknown
repository SHALL fail before a new inspection begins.

#### Scenario: Assessment cites reproducible source

- **WHEN** a clarification session reads repository X at commit `abc123`
- **THEN** its assessment evidence includes X and the exact full SHA `abc123`
