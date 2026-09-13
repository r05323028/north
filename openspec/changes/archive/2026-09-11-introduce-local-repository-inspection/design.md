# Design

## Context and boundaries

Configured repositories provide durable metadata and an enabled-only active
catalog. The server assembles complete `session.start` context and persists
session ownership/context before dispatch. The protocol carries repository
metadata and later `repository_id`/`commit_sha` facts but never accesses
repository persistence. This change supplies the daemon-side source material
and checkout used by the runtime.

## Authorization phases

1. For a new run, `north-server` reads the active catalog (`disabled_at IS
   NULL`), validates the selected IDs in the same durable start path, and
   stores the run's repository IDs before dispatching `session.start`.
2. The repository set in that persisted run context is immutable for the run.
   The daemon accepts inspection only for an ID present in that context; it
   never invents an ID or independently fetches the catalog.
3. A later disable changes active-catalog selection only. It does not cause the
   daemon to reject an already-authorized run, and readiness does not require
   the row to be enabled when its in-flight citation arrives.
4. Server readiness still requires the retained row, run binding, and all
   normal readiness/domain gates. There is no separate provenance database or
   subsystem.

A disabled or unknown repository can therefore fail a new selection without
invalidating a valid in-flight citation.

## Cache and checkout flow

- Store one reusable Git source cache under the daemon configuration directory,
  keyed by durable `repository_id`. The cache is reusable source material,
  never a runtime working directory. Mirror clones land first in a direct
  `.source-*` staging child; safe immediate cleanup handles failed clones, while
  startup recovery scans only recognized staging children in encoded cache
  namespaces.
- Hold a keyed per-repository synchronization lock across clone/fetch/update,
  exact revision resolution, and creation/verification of the disposable
  checkout. Sessions for different repositories proceed independently. Sessions
  for one repository wait for its cache operation, then run concurrently from
  different workspaces. North 0.1 assumes one daemon process owns its cache;
  that process must not allow two cache mutators for one repository ID.
  Staging cleanup revalidates cache-root identity, namespace boundaries, and
  staging identity; symlinks, path redirection, and uncertain ownership are
  retained and reported rather than deleted.
- Create a plain local clone/copy under a dedicated disposable-workspace root,
  scoped by session/task/repository identity. Do not use Git worktrees in 0.1.
- Release the cache lock only after the workspace is independent and its pinned
  revision has been verified. The runtime receives the disposable checkout path
  only; the cache path is never part of the runtime input.

## Deterministic revision pinning

While preparing the checkout, resolve the source with Git and capture the full
output of `git rev-parse HEAD` (or an equivalent verified commit resolution),
not an abbreviated ref. Check out that exact object in detached mode, then run
`git rev-parse HEAD` inside the disposable workspace and require equality with
the captured SHA. Do not let the runtime follow a remote branch or re-resolve a
ref after inspection begins.

A remote branch may move after preparation starts; the run still observes the
captured object. The inspection result carries the same durable repository ID
and full SHA. `north-protocol` validates complete Git SHA-1/SHA-256 fields; the daemon
produces the canonical full-SHA fact and server readiness validates identity/run
binding.

`LocalRuntime` owns the initialized `RepositoryInspector` only as an adapter
injection seam. Its current durable dispatch path intentionally returns
`runtime_adapter_not_configured`; this change does not invoke inspection from
production agent dispatch. The downstream
`introduce-agent-requirement-clarification` change owns the real runtime
adapter and its invocation.

## Workspace protection and lifecycle

The existing read-class Git allowlist and process-level dirty-tree guard remain
in force. The daemon checks the workspace for unexpected changes after every
inspection task. A dirty result is an invariant violation: report it, do not
reuse the workspace, and discard it. This detects contamination after the fact;
it is not OS/kernel read-only enforcement or a sandbox.

Every workspace is cleaned in a finally-style path:

| Outcome | Required behavior |
| --- | --- |
| Successful inspection | Check the dirty guard, publish the result, then remove the workspace. |
| Git/inspection failure | Report the failure, best-effort dirty check, then remove the workspace. |
| Cancellation | Stop/await the runtime task, check/discard the workspace, then remove it; no unfinished result is published. |
| Runtime failure | Preserve the runtime failure fact, apply the dirty guard, and remove the workspace. Contamination is reported separately. |
| Cleanup failure | Never reuse the directory. Leave it under the disposable root for safe stale cleanup and surface the cleanup failure. |
| Daemon restart | Workspace cleanup scans only the dedicated disposable-workspace root and removes stale, clearly identified disposable directories on a best-effort basis. A separate cache-staging pass scans only encoded cache namespaces and `.source-*` direct children. Neither pass deletes reusable `source.git`; unsafe paths remain and report failure. |

Startup cleanup is not a new evidence/provenance mechanism. Each pass is
filesystem hygiene bounded to its own daemon-owned namespace; cache-staging and
disposable-workspace cleanup remain separate.

## Dependency graph

```text
introduce-requirement-board
  -> base browser GET /events + requirement.changed (independent)
introduce-local-repository-inspection
  -> clarification orchestration
introduce-requirement-board + introduce-agent-requirement-clarification
  -> conversation/detail consumer
```

The later retry/failure change may extend runtime status handling but is not
needed to prepare repositories or render the initial clarification UI.
