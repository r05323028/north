# Design

## Context

A repository-local cache is useful for fetch/reuse but cannot be a mutable
runtime workspace when two sessions inspect the same repository. The cache and
checkout boundary is fixed by `harden-distributed-system-architecture`.

## Decisions

- Cache root under daemon config dir with one source cache per repository id;
  clone if missing and refresh through host `git` when clean. The runtime never
  receives this cache path as its working directory.
- Each session/task creates a unique disposable plain checkout from the cache
  (local copy/clone is sufficient). No Git worktrees in 0.1.0.
- All Git invocation uses host `git` with inherited SSH/config/credential-helper
  environment. No bundled auth logic and no credential serialization to North.
- Before/after task checks use repository status; any dirty result is an
  invariant violation, is reported, and the disposable checkout is discarded.
  This is process-level enforcement, not kernel/sandbox isolation.
- Inspection result = `{repository_id, commit_sha, notes}`; SHA comes from
  `git rev-parse HEAD`. The server rejects disabled/unknown repository ids
  before dispatch.

## Risks / Trade-offs

- **Copying a large repository costs local disk/time** → reuse the cache as
  source material; optimize only after measurement.
- **A process can still mutate files before detection** → discard and report
  dirty checkouts, and keep the non-sandbox limit explicit.
