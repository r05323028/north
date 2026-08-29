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
disabled repositories. Every readiness citation SHALL resolve to an existing
durable configured-repository row; an unknown identity is rejected before
accepted evidence or Requirement promotion. Evidence from an in-flight
inspection may remain valid after disable when the retained row exists and the
citation was present in the server-assembled session context or otherwise
explicitly inspected under that run. Readiness owns evidence acceptability;
configured repositories own identity/lifecycle. Server-side repository
configuration SHALL contain no Git credentials, tokens, keys, or passwords;
credentials remain in the daemon host's Git environment.

#### Scenario: Remove preserves assessment history

- **WHEN** an Admin removes repository X after an assessment recorded X at commit `abcdef0123456789abcdef0123456789abcdef01`
- **THEN** X is disabled rather than deleted, the old assessment still resolves to X and `abcdef0123456789abcdef0123456789abcdef01`, and new inspections cannot select X

#### Scenario: Repository credentials stay local

- **WHEN** the server persists configured repository metadata
- **THEN** its schema contains identity/URL/description and lifecycle metadata only, with no credential material

### Requirement: Inspections cite exact source revisions

Every successful inspection SHALL report the configured repository identity and
full resolved commit SHA from its disposable checkout. A disabled or unknown
repository SHALL fail before a new inspection begins. This selection rule does
not invalidate an already-authorized in-flight inspection solely because its
retained repository row becomes disabled; the citation remains eligible for
readiness acceptance subject to durable row existence and session/run provenance.

#### Scenario: Unknown citation cannot become evidence

- **WHEN** readiness receives a citation for an unknown repository identity
- **THEN** it durably rejects the assessment, does not promote the Requirement,
  and does not fabricate a repository row

#### Scenario: Disable during inspection preserves historical identity

- **WHEN** R is enabled in `session.start`, inspection begins, an Admin disables
  R, and the session later reports R plus its exact commit SHA
- **THEN** new inspections exclude R, while the in-flight citation remains
  eligible because the retained row and session/run provenance still exist

#### Scenario: Assessment cites reproducible source

- **WHEN** a clarification session reads repository X at commit `abcdef0123456789abcdef0123456789abcdef01`
- **THEN** its assessment evidence includes X and the exact full SHA `abcdef0123456789abcdef0123456789abcdef01`
