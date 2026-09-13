# Repository inspection

## Purpose

Lets the daemon inspect enabled configured repositories through host Git while
keeping every mutable checkout isolated and every readiness citation tied to an
exact source revision.

## ADDED Requirements

### Requirement: Host-environment Git is the credential boundary

Inspection SHALL invoke the host's normal `git` binary and environment,
including configured SSH, agent, credential helpers, authenticated host tools,
and file permissions. North SHALL not accept, serialize, or persist repository
credentials. A host-Git failure SHALL remain an inspection failure with its
operational error handled by the daemon boundary.

#### Scenario: Shell-equivalent access

- **WHEN** the daemon host can clone or fetch the configured URL with its normal Git environment
- **THEN** inspection uses that access without any North-managed credential field

### Requirement: New selection is enabled-only, but authorized runs survive disable

A new session context or new inspection selection SHALL accept only an enabled
retained repository ID. The server SHALL exclude disabled rows while assembling
new `session.start` context and SHALL reject unknown IDs. If repository R was
included while enabled in a persisted session/run context and inspection began,
then disabling R SHALL NOT cancel that run or invalidate its later citation
solely because `disabled_at` is now set. Historical acceptance still requires
the retained row, session/run binding, exact SHA, and normal readiness gates.

#### Scenario: Disabled repository is excluded from new selection

- **WHEN** a new session is assembled after repository R is disabled
- **THEN** R is not selected and no new checkout or inspection starts for R

#### Scenario: In-flight citation survives disable

- **WHEN** enabled R is included in `session.start`, inspection begins, an Admin disables R, and the same run later reports R with its exact full SHA
- **THEN** the run may complete and readiness may accept the citation; disable affects future selection only

#### Scenario: Unknown identity never starts work

- **WHEN** a requested repository ID is absent from the retained catalog or from the session-bound repository set
- **THEN** selection/inspection fails before cache access, workspace creation, or evidence publication

### Requirement: Cache mutation is synchronized per repository

A reusable cache SHALL have per-repository synchronization covering clone,
fetch, update, exact revision resolution, and creation/verification of a
workspace source snapshot. Concurrent sessions MAY inspect the same repository,
but SHALL NOT race cache mutation or share a mutable checkout. Synchronization
for repository R SHALL NOT unnecessarily serialize unrelated repository IDs.

#### Scenario: Same repository waits at cache boundary

- **WHEN** sessions A and B prepare repository R concurrently
- **THEN** one cache operation runs at a time for R, while each session receives a distinct independent workspace after preparation

#### Scenario: Different repositories remain independent

- **WHEN** sessions prepare repositories R and S concurrently
- **THEN** work for S is not blocked by a lock held only for R

### Requirement: Abandoned cache staging is safely recoverable

Mirror preparation SHALL use only explicitly recognized staging names such as
`.source-*` beneath an encoded repository cache namespace. If mirror cloning
fails after creating staging material, the daemon SHALL remove it immediately
when cache-root ownership, direct-child boundaries, and filesystem identity can
be proven. Startup recovery MAY remove stale staging left behind by an earlier
process using the same checks. If ownership, identity, or path boundaries
cannot be proven, the daemon SHALL retain the path and report cleanup failure.
Staging cleanup SHALL be separate from disposable-workspace cleanup, SHALL never
remove `source.git`, SHALL not follow symlinks, and SHALL never delete outside
the daemon-owned cache root.

#### Scenario: Failed mirror clone leaves staging material

- **WHEN** a mirror clone fails after creating `<namespace>/.source-*`
- **THEN** the daemon removes that staging directory when its identity and
  cache-root ownership still validate, while returning the Git inspection
  failure

#### Scenario: Startup removes safe stale staging

- **WHEN** startup finds a real stale `.source-*` direct child of an encoded
  repository cache namespace
- **THEN** it removes only that staging directory and reports it as removed

#### Scenario: Reusable cache and unrelated entries survive

- **WHEN** startup scans cache namespaces containing `source.git`, unrelated
  cache-root entries, or names outside the recognized staging layout
- **THEN** it leaves all of them untouched

#### Scenario: Staging redirection is rejected

- **WHEN** a recognized staging path or its repository namespace is a symlink,
  redirects outside its expected parent, or changes filesystem identity during
  cleanup
- **THEN** the daemon retains the path, reports cleanup failure, and does not
  follow or delete the redirected target

#### Scenario: Cache-root replacement is rejected

- **WHEN** the configured cache root no longer has its captured identity or
  canonical path before staging cleanup
- **THEN** the daemon reports cleanup failure and removes nothing from either
  the replacement root or the displaced cache tree

### Requirement: Runtime is confined to an isolated disposable workspace

The reusable cache SHALL be source material only. Each inspection SHALL run in a
unique disposable checkout scoped to session/task/repository identity. The
runtime SHALL never receive or operate directly inside the cache. A checkout
contaminated by an unexpected mutation SHALL be reported and discarded. North
0.1 SHALL use plain clone/copy isolation and SHALL not require Git worktrees or
claim OS/kernel sandboxing.

#### Scenario: Concurrent workspaces cannot contaminate one another

- **WHEN** sessions A and B inspect the same repository concurrently
- **THEN** their mutable directories differ, a mutation in A is not visible to B, and neither runtime can mutate the reusable cache

#### Scenario: Runtime receives checkout, not cache

- **WHEN** the daemon invokes the runtime
- **THEN** the runtime input contains only the session/task disposable workspace path and never the reusable cache path

### Requirement: Inspection pins one exact full commit SHA

Before runtime inspection begins, or during construction of its disposable
checkout, the daemon SHALL resolve one exact full commit SHA from Git, check out
that object in detached mode, and verify the workspace's `git rev-parse HEAD`
equals the captured value. The runtime SHALL not follow a branch after pinning.
The inspection result SHALL carry the configured `repository_id` and that same
full SHA. Full means Git's complete object ID, not an abbreviated ref; the
contract SHALL not assume one fixed hash width.

#### Scenario: Moving remote branch cannot change one run

- **WHEN** remote branch R changes after a run resolves commit C
- **THEN** that run's workspace remains at C and its readiness evidence cites C

#### Scenario: Evidence names exact source

- **WHEN** inspection of repository R succeeds at commit C
- **THEN** the typed result contains `repository_id = R` and the complete SHA C

### Requirement: Workspace cleanup covers every terminal path

Normal workspaces SHALL be disposed after successful, failed, cancelled, and
runtime-failure inspections. Cancellation SHALL await/stop the task before
cleanup where possible. A cleanup failure SHALL make the directory unavailable
for reuse and SHALL remain eligible for stale cleanup. Daemon startup MAY remove
stale disposable workspaces, but SHALL scan only the dedicated disposable root
and SHALL never delete reusable caches.

#### Scenario: Restart removes stale disposable directories only

- **WHEN** daemon startup finds an orphan under the known disposable-workspace root
- **THEN** it may remove that orphan best-effort while leaving every reusable repository cache untouched

#### Scenario: Cleanup failure is non-reuse

- **WHEN** workspace removal fails after inspection
- **THEN** the daemon reports the cleanup failure and never assigns that directory to another run

### Requirement: Dirty-tree guard remains process-level

The daemon SHALL retain read-class Git operations and the existing post-task
dirty-tree check. Any unexpected working-tree change SHALL be treated as an
inspection invariant violation, reported, and followed by workspace disposal.
This guard SHALL be documented as process-level detection/response, not an
OS/kernel sandbox or a guarantee that mutation was impossible.

#### Scenario: Dirty checkout is discarded

- **WHEN** a runtime leaves an unexpected change in its disposable checkout
- **THEN** the daemon reports the violation, emits no successful inspection result or readiness citation from that checkout, and discards it rather than reusing it
