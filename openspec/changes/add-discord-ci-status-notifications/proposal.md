# Discord CI status notifications

## Why

CI and pull-request lifecycle results currently require opening GitHub. Maintainers need status in Discord, while notification delivery must stay separate from the `gate` result so an external chat service cannot change merge correctness.

## What Changes

- **Invariant:** Add a GitHub Actions workflow that posts one concise Discord notification after the `CI` workflow completes or on pull request actions `opened`, `synchronize`, `reopened`, `ready_for_review`, and `review_requested`, including conclusion or action, branch or pull request context, and link.
- **Invariant:** Read webhook URL from repository secret `DISCORD_CI_WEBHOOK`; never hard-code credentials or print them.
- **Invariant:** Notification failure or missing configuration must not change the already-computed CI merge gate result.
- **Implementation suggestion:** Use runner-provided `curl` and `jq` with bounded retries; do not add a third-party action dependency.
- Document secret setup, event coverage, payload, and failure isolation in `docs/development/ci.md`.

Out of scope: changing CI validation jobs, required checks, branch protection, Discord commands, or historical backfill notifications.

## Capabilities

### New Capabilities

- `ci-status-notifications`: Notify Discord when the repository `CI` workflow completes or a selected pull request lifecycle event occurs.

### Modified Capabilities

None.

## Impact

- Adds `.github/workflows/discord-ci-status.yml` and one repository secret configuration requirement.
- Updates `docs/development/ci.md`.
- Depends on the existing `.github/workflows/ci.yml`; no earlier OpenSpec change is required.
- No application runtime, database, API, or production dependency changes.
