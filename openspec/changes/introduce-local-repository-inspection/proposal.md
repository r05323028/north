# Introduce local repository inspection runtime

## Why

Readiness assessments must cite real source without giving a runtime a shared,
mutable working tree. The daemon needs host-Git access that behaves like the
operator's shell, reusable source caches, isolated disposable workspaces, and
exact source-revision evidence.

## Scope and ownership

This change is downstream of `introduce-configured-repositories` and the
implemented server↔daemon protocol. It owns daemon-side source preparation and
inspection only:

- host `git` invocation using the daemon host's normal SSH/config/agent and
  credential-helper environment;
- one reusable source cache per configured repository;
- per-repository cache synchronization;
- unique session/task workspaces isolated from one another and from caches;
- process-level dirty-tree detection and workspace disposal; and
- full commit-SHA resolution and typed inspection evidence.

The server remains the authority for selecting repositories and assembling
`session.start`. The daemon does not fetch a repository catalog.

## Selection, authorization, and evidence

These are separate contracts:

| Moment | Rule | Owner |
| --- | --- | --- |
| Selection-time eligibility | A new session context or new inspection selection may include only an enabled retained repository. Unknown IDs are rejected. | `north-server`/persistence and this inspection boundary |
| Session/run-bound authorization | Once an enabled repository is included in the persisted `session.start` context for a pinned run, that repository remains authorized for that run. | server session ownership plus daemon inspection |
| Historical evidence validity | A citation remains valid after disable when the durable row still exists, the citation belongs to that authorized run, and it carries the exact full commit SHA. | server readiness/persistence |

Disabling a repository prevents future selection. It does not revoke legitimate
in-flight inspection or invalidate its evidence retroactively. The retained
repository row is still required. An unknown repository identity is never
accepted at any phase.

## What Changes

- Prepare a reusable per-repository cache, serialized by repository ID, then
  create a unique disposable checkout for each inspection.
- Resolve and pin one exact full commit SHA before runtime inspection. The
  runtime works from that detached revision and reports the same
  `repository_id` + SHA in readiness evidence.
- Keep the existing read-class Git and post-task dirty-tree guard. A dirty
  checkout is an invariant violation, not proof of OS-level sandboxing.
- Dispose workspaces on normal completion, inspection error, cancellation, and
  runtime failure. Startup may safely clean stale disposable directories under
  the dedicated workspace root without touching reusable caches.

## Explicit non-goals

No Git worktrees, pushing, PR creation, branch-selection UI, source mutation,
custom credential storage, new provenance subsystem, or OS/kernel sandbox is
introduced. The runtime never receives or operates in the reusable cache.

## Capabilities

### New Capabilities

- `repository-inspection`: host-Git inspection, synchronized source caches,
  exact revision pinning, isolated workspaces, dirty-tree protection, and
  lifecycle cleanup.

### Modified Capabilities

- `readiness`: accepted repository citations use inspection-produced full SHAs;
  readiness still owns acceptance and the atomic Requirement transition.

## Impact and dependencies

- Upstream: configured repository identity/lifecycle and active-catalog reads;
  `north-protocol` typed frames and durable delivery; existing readiness
  validation.
- Downstream: `introduce-agent-requirement-clarification` invokes this
  capability and consumes its inspection facts.
- This change does not own browser APIs, session retry policy, or Requirement
  business rules.

Dependency position:

```text
introduce-requirement-board
  └─ base authenticated GET /events + requirement.changed

introduce-local-repository-inspection
  └─> introduce-agent-requirement-clarification
       └─ extends Board's shared /events categories

introduce-requirement-board + introduce-agent-requirement-clarification
  └─> introduce-requirement-conversation-ui
```

Board's base browser invalidation is independent of repository inspection and
clarification runtime. `introduce-runtime-retry-and-failure-state` is a later
extension, not a prerequisite for this change or its consumers.
