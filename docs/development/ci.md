# CI

Remote GitHub Actions is the authoritative merge gate; local runs are parity
checks, never replacements.

## Workflow (`.github/workflows/ci.yml`)

| Job | Purpose |
| --- | --- |
| `pr-title` | PR title must be a Conventional Commit (squash-merge makes it the canonical subject on `main`) |
| `rust` | fmt --check · clippy `-D warnings` · unit tests + architecture checks |
| `rust-coverage` | Rust workspace LCOV coverage upload |
| `daemon-integration` | PostgreSQL-backed requirements, conversations, readiness, daemon lifecycle, repository, runtime activity retention, and durable protocol integration tests |
| `web` | lint · typecheck · production build (`apps/web`) · static docs-site build (`web/`) |
| `web-e2e` | Playwright Board/List/create/detail and SSE browser-boundary workflows on `ubuntu-latest` |
| `web-coverage` | Frontend Vitest LCOV coverage upload |
| `openspec` | `openspec validate --all --strict` |
| `gate` | succeeds only when all required jobs, including web E2E and coverage jobs, succeed |

The `main` ruleset requires exactly three checks: **`Rust (fmt, clippy, unit+architecture)`**,
**`PR title (Conventional Commit)`**, and **`merge gate`** (workflow job ID `gate`).
The PR-title check uses a separate lightweight workflow and reruns on `edited`;
edited events do not rerun full CI.

## Release workflow (`.github/workflows/release.yml`)

The workflow qualifies protected-main merges and publishes OCI images only from strict SemVer tag releases; it does not deploy North.

Protected-main pushes build and qualify immutable package/OCI artifacts; they create no GHCR image refs:

```text
CI qualification → build-package (package + OCI)
  → release-preflight (source, artifact ID + digest, checksums)
  → release-qualification (fresh hosted runner, test-only TLS)
 ```

Every successful main merge qualifies exact package/OCI inputs but publishes no image to GHCR. Package and OCI artifacts stay in a 14-day workflow artifact; preflight and qualification verify the same artifact ID/digest and exercise the exact OCI images. Main OCI digests remain internal qualification evidence; only strict tag builds publish SemVer refs. Only the `release-qualification` provisioning step reads `NORTH_RELEASE_TLS_CA_PEM`, `NORTH_RELEASE_TLS_CERT_PEM`, and `NORTH_RELEASE_TLS_KEY_PEM`; no other job receives them and no GitHub Environments are involved.

A strict stable SemVer Git-tag push (`vX.Y.Z`) runs a fresh tag-build path:

```text
CI tag gate ─┬─ build-package + OCI → preflight → release-qualification ─┐
             └─ 3 native CLI archives → verify-cli-archives ────────────┤
                       draft CLI assets → SemVer OCI → finalize Release
```

The tag validator requires strict SemVer, a peeled tag target equal to
`GITHUB_SHA`, ancestry from `origin/main`, matching Cargo/CLI/web versions, and
release code current with `origin/main`. The workflow explicitly fetches all tag
refs; `initial_release=true` only when the event tag is the sole strict SemVer
ref. Later tags require a first-parent version bump. This checks current refs;
deleted historical tags cannot be detected. Missing or malformed validation
output blocks publication. Tag jobs
build new package/OCI inputs and native Linux x86_64, macOS x86_64, and macOS
ARM64 CLI pairs from that commit; they never reuse main qualification archives. Preflight and
hosted Compose qualification verify the same package/OCI artifact ID and digest.
Each CLI archive contains the matching binaries, version/target/source metadata,
and SHA-256 checksums; both binaries run `--version` on native builders. The
CLI publisher uploads only to a draft GitHub Release. Retries accept byte-identical
assets and fail on conflicts without clobbering. The GHCR publisher preflights
both SemVer destinations before copying either exact qualified OCI archive,
verifies registry digests, and accepts only matching existing tags. Release
finalization waits for both image publications; GHCR failure leaves draft private.

Only `publish-semver-images` receives `packages: write`; main and manual qualification have no GHCR publisher permission. Draft upload and finalization jobs receive `contents: write` only; no publisher receives TLS secrets. The SemVer publisher accepts an existing tag only when its registry digest matches the qualified OCI digest. Workflow dispatch with a full `source_sha` qualifies only. Strict tag validation rejects prereleases and build metadata. GHCR visibility remains an owner setting; operators should pin registry digests. Signing, provenance, and SBOM remain deferred. Operator installation, archive verification, `north setup`, and daemon lifecycle commands are documented in [`docs/deployment/self-hosted.md`](../deployment/self-hosted.md).

## PR-Agent advisory review

North includes an advisory PR-Agent workflow at
`.github/workflows/pr-agent.yml`. It reviews pull requests on
`opened`, `synchronize`, `reopened`, `ready_for_review`, and
`review_requested` events, but it is not part of `gate` and must not be added
as a required branch-protection check.

Repository administrators must add an `OPENCODE_API_KEY` Actions secret under
**Settings → Secrets and variables → Actions**. The workflow maps that secret
to PR-Agent's `OPENAI_KEY` input and routes
`openai/mimo-v2.5` through OpenCode Go's Chat Completions endpoint. The
workflow supplies `LITELLM.EXTRA_HEADERS` with a stable per-PR
`x-opencode-session` and identifies itself as `north-pr-agent/1.0`, as required
by OpenCode Go. The workflow uses `pull_request_target` so fork pull requests
can use that secret. It does not checkout or execute pull-request code, grants
only `contents: read`, `issues: write`, and `pull-requests: write`, and pins
PR-Agent to release `v0.44.0` by commit SHA. Review `the-pr-agent/pr-agent`
before changing that pin.

Because `pull_request_target` runs the workflow from the base branch, PR-Agent
could otherwise merge repository-controlled settings from the default branch's
`.pr_agent.toml`. The target workflow therefore pins provider/model/reviewer
settings in container-compatible `CONFIG__*`, `OPENAI__*`, and `LITELLM__*`
environment keys and sets `CONFIG__USE_REPO_SETTINGS_FILE=false`, which
disables that lookup entirely. The workflow environment is the sole trusted
provider source for this job: a stale or PR-influenced `.pr_agent.toml` is
never an input. `.pr_agent.toml` remains the manual/local PR-Agent
configuration and must stay consistent with the workflow pins; structural
tests in `tests/architecture/tests/architecture.rs` enforce both properties.

PR-Agent review is advisory. To roll it back, disable/remove the workflow and
revoke `OPENCODE_API_KEY`; existing CI and `gate` remain unchanged.

## Required repository settings

GitHub branch protection / ruleset for `main`:

- Require pull request before merging and at least one approving review.
- Require the existing checks: **`Rust (fmt, clippy, unit+architecture)`**,
  **`PR title (Conventional Commit)`**, and **`merge gate`**.
- Restrict pushes to reviewed PR merges; block direct pushes and bypass actors.
- Require linear history and allow squash merge only.
- Require branches up to date before merging (not currently enforced; see live ruleset readback below).

GitHub tag ruleset for SemVer releases (`v*`):

- Restrict tag creation, updates, and deletion to authorized release maintainers.
- Prevent bypass; tags must target a main-reachable commit on protected `main`.
- The initial strict tag may reuse its first-parent version only when no other strict tag ref currently exists; subsequent tags must target reviewed version-bump commits. The tag workflow runs its own qualification and builds fresh release artifacts.

Coverage jobs use `fail_ci_if_error: true`, so `gate` fails when coverage is not
generated or uploaded. Codecov separately evaluates project and flag statuses;
workflow code does not parse percentages. Patch status is temporarily disabled
while baseline coverage is established. When re-enabled, restore patch `>= 80%`
and add `codecov/patch` to the default-branch ruleset required status checks
after it reports successfully. Do not require global/project Codecov status yet;
project target remains `auto`, with allowed project regression `1%`.

Enforcement verified 2026-09-28 via active repository ruleset `ruleset-default`
(id `21581438`, applies to default branch, no bypass actors):
`required_approving_review_count=1`; required contexts are `Rust (fmt, clippy,
unit+architecture)`, `PR title (Conventional Commit)`, and `merge gate`. Deletion,
non-fast-forward, and linear-history protections remain active; merge method is
`squash` only.
`strict_required_status_checks_policy=false`, so branch freshness remains an owner action.

Recheck repository/spec consistency with:

```sh
gh api repos/r05323028/north/rulesets/21581438 --jq '{
  approving_reviews: ([.rules[] | select(.type == "pull_request")][0].parameters.required_approving_review_count),
  required_checks: ([.rules[] | select(.type == "required_status_checks")][0].parameters.required_status_checks | map(.context)),
  merge_methods: ([.rules[] | select(.type == "pull_request")][0].parameters.allowed_merge_methods),
  protection_rules: ([.rules[].type] | sort),
  bypass_actors: .bypass_actors
}'
```

Expected: one approval, three named checks above, squash-only, `deletion`,
`non_fast_forward`, and `required_linear_history` protections, and no bypass actors.
YAML cannot enforce this repository-level state.

## Local parity

`./scripts/pre-push-validation.sh` builds a test-only Testcontainers runner,
starts disposable PostgreSQL 16, and passes its dynamic URL to the native
`./scripts/validate.sh ci` child. It removes the container before replaying a
real workflow job through [act](https://github.com/nektos/act). Docker must be
running; no manually configured database URL is needed for pre-push. Direct
`./scripts/validate.sh ci` and `integration` invocations still require
`NORTH_TEST_DATABASE_URL`. SIGINT/SIGTERM stop the child and remove the\ncontainer. SIGKILL or host loss can leave a tmpfs-only container, never a\npersistent volume. The `daemon-integration` job delegates its database suites to\n`./scripts/validate.sh integration` with its hosted PostgreSQL service.\n
Known limitations of act parity (documented, not hidden):

- The default `rust` job targets `self-hosted`; local Act may print `Skipping unsupported platform`. The pre-push hook treats that as failure because no workflow parity ran.
- Until a working Act platform mapping exists for this job, the supported native-only local command is `NORTH_PRE_PUSH_SKIP_ACT=1 ./scripts/pre-push-validation.sh`. Testcontainers runs native `ci`; this is not workflow parity.
- Actions needing GitHub API context or hosted-runner networking behave
  differently locally;
- container images drift from `ubuntu-latest` VMs;
- act validates job steps, not branch-protection semantics.

Red remote CI always wins over green local output.

## Discord notifications

`.github/workflows/discord-ci-status.yml` listens for completed runs of `CI` and
for pull request actions `opened`, `synchronize`, `reopened`, `ready_for_review`,
and `review_requested`. Add repository Actions secrets
`DISCORD_CI_WEBHOOK` for generic CI notifications and `DISCORD_PR_WEBHOOK` for
review-channel notifications. Set both to normal Discord webhook URLs. Do not
append `/github` to `DISCORD_PR_WEBHOOK`; the workflow strips that legacy suffix
if it is still present during migration.

Generic CI notifications include conclusion, triggering event, head branch, run
number, actor, and a link to the completed run. Pull request lifecycle events
produce linked embeds with repository, action, PR number/title, author, and
source branch. A completed `CI` run associated with a PR additionally produces
a review embed like `[repo] Checks Successful on PR: #<number> <title>` with
linked `PR Author` and `Workflow Run` fields plus `Source Branch`. Failure,
cancellation, timeout, and action-required conclusions use non-success colors.

The workflow uses only event metadata and a read-only `pull-requests: read`
permission to resolve PR title and author for completed PR runs. It does not
check out or execute pull request code. Missing event-specific webhook
configuration skips that notification and succeeds. Configured webhook or
metadata delivery failure is visible after bounded retries, but never changes
`CI` or its required `gate` result. `workflow_run` notifications start after
this workflow exists on the default branch. Pull request events from forks
cannot access repository secrets and therefore skip safely. Remove the secret
or delete this workflow to roll back; existing CI validation remains unchanged.

Local `act` parity does not replay `workflow_run` completion delivery or real
Discord/remote pull request events. Validate notification behavior with a
completed remote CI run and a selected pull request event after configuring the
secret.
