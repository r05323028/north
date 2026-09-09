# Use Discord PR embeds

## Why

Current pull-request notifications use Discord's GitHub-compatible `/github` endpoint, which does not provide the compact, review-focused embed layout maintainers want in `#pr-review`. Replace that rendering with a standard Discord webhook embed while retaining non-blocking notification behavior.

## What Changes

- Replace raw GitHub event forwarding for `DISCORD_PR_WEBHOOK` with a standard Discord embed.
- Render pull-request lifecycle events with repository, PR number/title, author, action, branches, and link.
- Also render completed CI runs associated with a pull request as a checks-status embed matching the supplied design: status title, run description, PR author, workflow run, and source branch.
- Accept `DISCORD_PR_WEBHOOK` as a normal Discord webhook URL; tolerate an old trailing `/github` suffix during migration.
- Keep `DISCORD_CI_WEBHOOK` generic CI notifications and keep all Discord delivery isolated from CI's required gate.

Out of scope: changing CI jobs, branch protection, repository permissions beyond read-only PR metadata, or adding a Discord bot.

## Capabilities

### New Capabilities

- `ci-pr-embeds`: Rich, review-focused Discord embeds for pull-request lifecycle events and CI checks.

### Modified Capabilities

- None. The prior notification capability is not yet synchronized into canonical `openspec/specs/`; this change adds the embed contract without editing that active change.

## Impact

- `.github/workflows/discord-ci-status.yml` builds standard Discord embed JSON and may read public PR metadata through the GitHub Actions token for workflow-run PR checks.
- `docs/development/ci.md` documents the new webhook URL shape and embed fields.
- `DISCORD_PR_WEBHOOK` repository secret should point to the normal webhook URL without `/github` after migration; old suffixed values remain accepted.
