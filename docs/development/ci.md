# CI

Remote GitHub Actions is the authoritative merge gate; local runs are parity
checks, never replacements.

## Workflow (`.github/workflows/ci.yml`)

| Job | Purpose |
| --- | --- |
| `pr-title` | PR title must be a Conventional Commit (squash-merge makes it the canonical subject on `main`) |
| `rust` | fmt --check · clippy `-D warnings` · unit tests + architecture checks |
| `daemon-integration` | PostgreSQL-backed requirements, conversations, readiness, and daemon lifecycle integration tests |
| `web` | lint · typecheck · production build (`apps/web`) |
| `openspec` | `openspec validate --all --strict` |
| `gate` | succeeds only when all required jobs succeed |

Branch protection should require exactly one check: **`gate`** — internal job
structure may evolve without touching rulesets.

## Required repository settings (owner applies)

GitHub branch protection / ruleset for `main`:

- Require pull request before merging.
- Require status check: **`gate`**.
- Require branches up to date before merging.
- (Recommended) Allow squash merge only, so PR titles stay the canonical history.

These settings cannot be changed from inside the repository by agents; until
they exist, treat a red `gate` as an absolute merge blocker regardless.

## Local parity

`./scripts/pre-push-validation.sh` runs native checks then replays one real
workflow job through [act](https://github.com/nektos/act) in Docker. The
`daemon-integration` job delegates its database suites to
`./scripts/validate.sh integration`.

Known limitations of act parity (documented, not hidden):

- Actions needing GitHub API context or hosted-runner networking behave
  differently locally;
- container images drift from `ubuntu-latest` VMs;
- act validates job steps, not branch-protection semantics.

Red remote CI always wins over green local output.
