# Design

## Decisions

- Workspace root under daemon config dir; one directory per repository id;
  clone if missing, `git fetch` + fast-forward read otherwise. Plain clones
  only in 0.1.0 (worktrees deferred until write features).
- All git invocation via host `git` binary with inherited environment
  (SSH agent, credential helpers). No bundled auth logic.
- Inspection result = {repository_id, commit_sha, notes}; SHA from `git rev-parse HEAD`.
- Read-only discipline: daemon issues only clone/fetch/rev-parse/log/grep-class
  commands; a deny-by-default command allowlist makes mutation structurally
  unlikely.

## Open Questions

None.
