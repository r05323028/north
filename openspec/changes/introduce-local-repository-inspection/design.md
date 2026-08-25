# Design

## Decisions

- Workspace root under daemon config dir; one directory per repository id;
  clone if missing, `git fetch` + fast-forward read otherwise. Plain clones
  only in 0.1.0 (worktrees deferred until write features).
- All git invocation via host `git` binary with inherited environment
  (SSH agent, credential helpers). No bundled auth logic.
- Inspection result = {repository_id, commit_sha, notes}; SHA from `git rev-parse HEAD`.
- Mutation discipline: deny-by-default read-class Git allowlist PLUS a
  disposable-checkout policy — post-task dirty-tree detection treats any
  direct file mutation as an invariant violation (discard + report). Honest
  scope: process-level enforcement, not kernel sandboxing.

## Open Questions

None.
