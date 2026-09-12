# Tasks

## 1. Selection and run authorization

- [x] 1.1 Reuse the server active-catalog/session-start path: only enabled retained repository IDs enter a new run; persist the run repository set before dispatch and reject unknown IDs.
- [x] 1.2 Preserve run-bound authorization after disable: an already-authorized inspection may finish and cite its retained repository, while future selection excludes it; readiness remains responsible for citation acceptance.
- [x] 1.3 Add boundary tests for enabled selection, unknown identity, disable-during-inflight inspection, and no credential fields crossing the server/daemon boundary.

## 2. Cache and checkout isolation

- [x] 2.1 Add per-repository cache layout and keyed synchronization covering clone/fetch/update plus source-snapshot preparation; unrelated repositories remain independent.
- [x] 2.2 Create unique session/task/repository disposable workspaces from cache source material; prove concurrent inspections never share mutable paths.
- [x] 2.3 Keep the runtime input on the disposable checkout path only; do not add Git worktrees or a cache-backed runtime mode.

## 3. Exact revision and dirty-tree protection

- [x] 3.1 Resolve and retain one full Git commit SHA (complete object ID), create a detached checkout at that object, verify `git rev-parse HEAD`, and carry the same repository ID/full SHA through inspection result and readiness evidence.
- [x] 3.2 Preserve the read-class Git allowlist and post-task dirty-tree guard; report contamination separately from runtime failure and discard the workspace.
- [x] 3.3 Add a moving-remote/ref fixture proving one run cannot silently observe a later branch revision.

## 4. Workspace lifecycle

- [x] 4.1 Dispose workspaces on success, Git/inspection failure, cancellation, and runtime failure through one cleanup path; never reuse a cleanup-failed directory.
- [x] 4.2 Add safe best-effort startup cleanup for stale disposable workspaces under the dedicated root; prove reusable caches are untouched.
- [x] 4.3 Test cleanup failures, cancellation, orphan recovery, and dirty-checkout disposal.
- [x] 4.4 Clean failed-clone and stale `.source-*` cache staging through a separate, identity- and boundary-checked recovery path; cover reusable-cache preservation, unrelated entries, symlink rejection, root replacement, and escape prevention.

## 5. Runtime boundary

- [x] 5.1 Name and document `LocalRuntime`'s initialized repository inspection
  field as future adapter infrastructure; keep production dispatch on the
  existing `runtime_adapter_not_configured` placeholder.

## 6. Validation

- [x] 6.1 Run focused repository-inspection/readiness integration tests and architecture checks.
- [x] 6.2 Run Rust/web validation required by the eventual implementation and `openspec validate --all --strict`.
