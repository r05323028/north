# Tasks

## 1. Selection and run authorization

- [ ] 1.1 Reuse the server active-catalog/session-start path: only enabled retained repository IDs enter a new run; persist the run repository set before dispatch and reject unknown IDs.
- [ ] 1.2 Preserve run-bound authorization after disable: an already-authorized inspection may finish and cite its retained repository, while future selection excludes it; readiness remains responsible for citation acceptance.
- [ ] 1.3 Add boundary tests for enabled selection, unknown identity, disable-during-inflight inspection, and no credential fields crossing the server/daemon boundary.

## 2. Cache and checkout isolation

- [ ] 2.1 Add per-repository cache layout and keyed synchronization covering clone/fetch/update plus source-snapshot preparation; unrelated repositories remain independent.
- [ ] 2.2 Create unique session/task/repository disposable workspaces from cache source material; prove concurrent inspections never share mutable paths.
- [ ] 2.3 Keep the runtime input on the disposable checkout path only; do not add Git worktrees or a cache-backed runtime mode.

## 3. Exact revision and dirty-tree protection

- [ ] 3.1 Resolve and retain one full Git commit SHA (complete object ID), create a detached checkout at that object, verify `git rev-parse HEAD`, and carry the same repository ID/full SHA through inspection result and readiness evidence.
- [ ] 3.2 Preserve the read-class Git allowlist and post-task dirty-tree guard; report contamination separately from runtime failure and discard the workspace.
- [ ] 3.3 Add a moving-remote/ref fixture proving one run cannot silently observe a later branch revision.

## 4. Workspace lifecycle

- [ ] 4.1 Dispose workspaces on success, Git/inspection failure, cancellation, and runtime failure through one cleanup path; never reuse a cleanup-failed directory.
- [ ] 4.2 Add safe best-effort startup cleanup for stale disposable workspaces under the dedicated root; prove reusable caches are untouched.
- [ ] 4.3 Test cleanup failures, cancellation, orphan recovery, and dirty-checkout disposal.

## 5. Validation

- [ ] 5.1 Run focused repository-inspection/readiness integration tests and architecture checks.
- [ ] 5.2 Run Rust/web validation required by the eventual implementation and `openspec validate --all --strict`.
