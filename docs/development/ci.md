# CI

Remote GitHub Actions is the authoritative merge gate; local runs are parity
checks, never replacements.

## Workflow (`.github/workflows/ci.yml`)

| Job | Purpose |
| --- | --- |
| `pr-title` | PR title must be a Conventional Commit (squash-merge makes it the canonical subject on `main`) |
| `rust` | fmt --check · clippy `-D warnings` · unit tests + architecture checks |
| `rust-coverage` | Rust workspace LCOV coverage upload |
| `daemon-integration` | PostgreSQL-backed requirements, conversations, readiness, daemon lifecycle, repository, and durable protocol integration tests |
| `web` | lint · typecheck · production build (`apps/web`) |
| `web-e2e` | Playwright Board/List/create/detail and SSE browser-boundary workflows on `ubuntu-latest` |
| `web-coverage` | Frontend Vitest LCOV coverage upload |
| `openspec` | `openspec validate --all --strict` |
| `gate` | succeeds only when all required jobs, including web E2E and coverage jobs, succeed |

Branch protection should require exactly one check: **`gate`** — internal job
structure may evolve without touching rulesets.

## PR-Agent advisory review

North includes an advisory PR-Agent workflow at
`.github/workflows/pr-agent.yml`. It reviews pull requests on
`opened`, `synchronize`, `reopened`, `ready_for_review`, and
`review_requested` events, but it is not part of `gate` and must not be added
as a required branch-protection check.

Repository administrators must add an `OPENCODE_API_KEY` Actions secret under
**Settings → Secrets and variables → Actions**. The workflow maps that secret
to PR-Agent's `OPENAI_KEY` input and routes
`openai/muse-spark-1.3-contributor` through OpenCode Go's OpenAI-compatible
endpoint. The workflow uses `pull_request_target` so fork pull requests can
use that secret. It does not checkout or execute pull-request code, grants
only `contents: read`, `issues: write`, and `pull-requests: write`, and pins
PR-Agent to release `v0.44.0` by commit SHA. Review `the-pr-agent/pr-agent`
before changing that pin.

PR-Agent review is advisory. To roll it back, disable/remove the workflow and
revoke `OPENCODE_API_KEY`; existing CI and `gate` remain unchanged.

## Required repository settings (owner applies)

GitHub branch protection / ruleset for `main`:

- Require pull request before merging.
- Require status check: **`gate`**.
- Require branches up to date before merging.
- (Recommended) Allow squash merge only, so PR titles stay the canonical history.

Coverage jobs use `fail_ci_if_error: true`, so `gate` fails when coverage is not
generated or uploaded. Codecov separately evaluates project and flag statuses;
workflow code does not parse percentages. Patch status is temporarily disabled
while baseline coverage is established. When re-enabled, restore patch `>= 80%`
and add `codecov/patch` to the default-branch ruleset required status checks
after it reports successfully. Do not require global/project Codecov status yet;
project target remains `auto`, with allowed project regression `1%`.

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
