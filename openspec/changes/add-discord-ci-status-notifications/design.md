# Discord CI status notifications

## Context

The existing `.github/workflows/ci.yml` runs validation and computes `gate` for pushes to `main` and pull requests. The repository also needs concise Discord notices for selected pull request lifecycle actions. See `proposal.md` for motivation. Chat delivery is an external side effect and must not become another required check.

## Goals / Non-Goals

**Goals:**

- Run one downstream notification workflow after each completed `CI` run and each selected pull request lifecycle action.
- Provide useful status and run context in Discord.
- Keep webhook credentials out of source and logs.
- Keep notification failures independent from merge correctness.

**Non-Goals:**

- Modifying existing CI jobs or `gate` dependencies.
- Adding application code, database state, or a Discord bot.
- Replaying historical runs or notifying on unrelated workflows.

## Decisions

1. **Use `workflow_run` and selected `pull_request` triggers.** The workflow listens for `CI` `completed` events and pull request actions `opened`, `synchronize`, `reopened`, `ready_for_review`, and `review_requested`. One notification job branches on event type to send either completed-run status or pull request lifecycle context. Keeping it separate from `ci.yml` avoids coupling an external service to the merge gate.

2. **Use runner-native `curl` and `jq`.** Construct the Discord JSON payload with `jq --arg` so branch and actor values remain data even when supplied by pull requests. `curl` uses bounded retries and timeouts. This avoids an unpinned third-party action and adds no dependency.

3. **Use separate optional secrets.** Use `DISCORD_CI_WEBHOOK` for `workflow_run` notifications and `DISCORD_PR_WEBHOOK` for pull request reminders. Select the corresponding secret from event type. Missing configuration emits an explicit skip message and succeeds; configured delivery errors remain visible on the notification workflow while never affecting the already-completed CI workflow.

4. **Grant no repository permissions.** The workflow only consumes `workflow_run` and `pull_request` metadata and sends an outbound webhook; it does not checkout code, execute pull request code, or call the GitHub API.

5. **Document repository setup.** `docs/development/ci.md` will name both secrets, explain event coverage and failure isolation, and describe local parity limits.

## Risks / Trade-offs

- [Webhook misconfiguration] → Missing secret is skipped; malformed or unreachable configured endpoints fail the notification job visibly.
- [Discord rate limiting or outage] → `curl` retries transient delivery failures with a short bound; CI remains unaffected.
- [Pull-request text reaches an external service] → Send only workflow metadata needed for status, encode all values with `jq`, and do not include source, tokens, or patch content.
- [Workflow trigger availability] → `workflow_run` requires this workflow to exist on the default branch before remote CI notifications begin; selected pull request events use the workflow revision available for the pull request event. Document both behaviors.

## Migration Plan

1. Merge the workflow and documentation.
2. Add repository Actions secrets `DISCORD_CI_WEBHOOK` for CI and `DISCORD_PR_WEBHOOK` for pull request reminders.
3. Confirm one completed CI run and one selected pull request event produce the expected messages.
4. Roll back by deleting the notification workflow or removing the secret; existing CI validation and `gate` remain unchanged.
