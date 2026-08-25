# Git workflow

## Branches and pull requests

- Work happens on feature branches; `main` receives changes through **pull
  requests**, not direct pushes.
- A PR is conceptually coherent: one change, not a grab-bag.
- Required CI (`gate`) must pass before merge.

## Conventional Commits

Format: `type(scope)?: description`

Allowed types: `feat fix docs test refactor perf build ci chore revert`.
Scope optional. Examples:

```text
feat(auth): add verification-code login
fix(domain): reject reopen from draft
docs(protocol): clarify resume handshake
test(domain): cover no-op requirement edits
ci: add merge gate
```

## Canonical history rule (squash merges)

North uses squash merging: the **PR title becomes the commit subject on
`main`**. Therefore:

- the PR title MUST follow Conventional Commits (enforced by CI `pr-title`);
- branch commits MAY be informal — they vanish at squash time;
- the local `commit-msg` hook (via prek) still validates subjects if you keep
  tidy branch history; it is enabled by default and intentionally easy to
  bypass when you want messy WIP commits locally.

Enforcement points:

- `scripts/check-commit-message.sh` — shared validator (hook + CI);
- `.pre-commit-config.yaml` `conventional-commit-msg` hook;
- GitHub `pr-title` job.
