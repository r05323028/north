## Purpose

Lets the daemon read enabled configured repositories with the host's existing Git setup, while concurrent clarification tasks use isolated disposable checkouts and cite exact commits.

## ADDED Requirements

### Requirement: Host-environment Git access

Repository access SHALL use the host's normal `git` binary and environment (SSH
config/agent, credential helpers). If cloning the URL works from the host shell,
inspection SHALL succeed without extra North credentials; otherwise it SHALL
fail with the underlying Git error surfaced.

#### Scenario: Shell-equivalent access

- **WHEN** `git clone <url>` succeeds in the daemon host shell
- **THEN** the daemon prepares readable cache/checkouts for that URL with no additional credentials provided by North

### Requirement: Every clarification gets an isolated disposable checkout

A reusable repository cache SHALL never be the runtime working tree. Each
clarification execution SHALL receive a unique checkout scoped to session/task
and repository id. Concurrent sessions inspecting one repository SHALL use
different mutable directories. Runtime changes MUST remain inside that
checkout; a contaminated checkout SHALL be discarded and reported. North
0.1.0 SHALL describe this as process-level protection, not kernel/sandbox
isolation, and SHALL not require Git worktrees.

#### Scenario: Concurrent inspections do not share files

- **WHEN** sessions A and B inspect repository X concurrently
- **THEN** each runtime sees its own disposable checkout and a mutation in A cannot appear in B or the reusable cache

#### Scenario: Dirty tree is a violation

- **WHEN** the daemon detects an unexpected working-tree change after a clarification task
- **THEN** it reports the violation and discards that checkout before reuse

### Requirement: Disabled repositories cannot start inspection

The server SHALL reject an inspection for an unknown or disabled repository
before dispatching work. New session catalogs SHALL include enabled metadata
only; repository credentials SHALL remain on the daemon host.

#### Scenario: Disabled selection fails

- **WHEN** a session requests inspection of a repository with `disabled_at` set
- **THEN** no checkout or runtime task starts and the server returns an unavailable-repository error

### Requirement: Inspections cite exact commits

Every successful inspection SHALL resolve and report the configured repository
identity and full commit SHA from its disposable checkout, making assessments
reproducible against exact source states.

#### Scenario: Assessment can name its basis

- **WHEN** the agent inspects repository X during clarification
- **THEN** the resulting event carries X and the full SHA returned by `git rev-parse HEAD`
