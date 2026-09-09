# Use Discord PR embeds

## Context

The existing notification workflow already separates Discord delivery from `CI.gate` and builds the generic CI message with runner-native `curl` and `jq`. Its PR branch forwards raw GitHub events to Discord's `/github` endpoint, which prevents the review channel from using the requested embed layout.

## Goals / Non-Goals

**Goals:**

- Use one `DISCORD_PR_WEBHOOK` secret for standard Discord embeds.
- Render selected PR lifecycle events and completed PR checks in a compact, linked layout matching the supplied screenshot.
- Preserve optional configuration, bounded retries, no checkout, and CI-gate isolation.

**Non-Goals:**

- Changing generic `DISCORD_CI_WEBHOOK` content.
- Adding a bot, dependency, database state, or GitHub write permission.
- Posting historical notifications or changing pull-request event coverage.

## Decisions

1. **Use the standard Discord webhook endpoint.** PR delivery sends an object containing `username` and `embeds`; it does not forward the GitHub envelope to `/github`. The script strips one legacy trailing `/github` suffix so the repository secret can migrate from the current value without an outage.

2. **Keep lifecycle and checks paths distinct.** `pull_request` events continue to provide immediate activity reminders. `workflow_run` continues to produce the generic CI notification and additionally produces a PR checks embed when its event is `pull_request`, matching the screenshot's `Checks Successful on PR` message.

3. **Read PR details with the Actions token.** A workflow-run payload identifies associated PR numbers but may omit title and author. For the first associated PR, make a read-only GitHub API request using `GITHUB_TOKEN` and `pull-requests: read`; no source checkout or pull-request code execution occurs.

4. **Build all external text with `jq --arg`.** Repository, title, author, branches, URLs, run names, and actions remain data. Status colors and labels are fixed shell mappings. The embed uses linked PR Author and Workflow Run fields and a source-branch field, with title truncation at Discord's 256-character limit.

5. **Reuse one bounded sender.** Both PR payload types use the existing `curl --fail-with-body --retry 3` timeout policy. A failed PR request can fail this notification workflow but cannot become a dependency of `CI`.

## Risks / Trade-offs

- [GitHub API metadata unavailable] → Skip the PR-specific checks embed with an explicit message; generic CI delivery remains attempted and CI remains unaffected.
- [Legacy secret still ends in `/github`] → Strip only the exact trailing suffix before standard delivery; document migration to the base URL.
- [Multiple PRs associated with one run] → Report first associated PR, matching one compact review notification per run; current CI runs normally map to one PR.
- [Long or untrusted PR text] → Pass values through `jq --arg` and truncate only the embed title to Discord's 256-character limit.

## Migration Plan

1. Merge workflow and documentation changes.
2. Replace repository `DISCORD_PR_WEBHOOK` with the base Discord webhook URL without `/github`; legacy suffixed values remain compatible during rollout.
3. Trigger one selected PR event and one completed PR CI run, then verify the embed in `#pr-review`.
4. Roll back by restoring the previous workflow and `/github` secret behavior; generic CI validation remains unchanged.
