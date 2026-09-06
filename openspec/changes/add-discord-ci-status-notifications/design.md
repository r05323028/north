# Discord CI status notifications

## Context

The existing `.github/workflows/ci.yml` runs validation and computes `gate` for pushes to `main` and pull requests. See `proposal.md` for motivation. Chat delivery is an external side effect and must not become another required check.

## Goals / Non-Goals

**Goals:**

- Run one downstream notification workflow after each completed `CI` run.
- Provide useful status and run context in Discord.
- Keep webhook credentials out of source and logs.
- Keep notification failures independent from merge correctness.

**Non-Goals:**

- Modifying existing CI jobs or `gate` dependencies.
- Adding application code, database state, or a Discord bot.
- Replaying historical runs or notifying on unrelated workflows.

## Decisions

1. **Use `workflow_run` completion trigger.** A separate workflow listening for the `CI` workflow's `completed` event sends status only after `gate` and every other CI job have concluded. Adding a job to `ci.yml` would couple an external service to the merge gate; polling or per-job notifications would produce partial or duplicate status.

2. **Use runner-native `curl` and `jq`.** Construct the Discord JSON payload with `jq --arg` so branch and actor values remain data even when supplied by pull requests. `curl` uses bounded retries and timeouts. This avoids an unpinned third-party action and adds no dependency.

3. **Use `DISCORD_CI_WEBHOOK` as optional secret.** Pass the secret through the step environment. Missing configuration emits an explicit skip message and succeeds; configured delivery errors remain visible on the notification workflow while never affecting the already-completed CI workflow.

4. **Grant no repository permissions.** The workflow only consumes the `workflow_run` event and sends an outbound webhook; it does not checkout code or call the GitHub API.

5. **Document repository setup.** `docs/development/ci.md` will name the secret, explain event coverage and failure isolation, and describe local parity limits.

## Risks / Trade-offs

- [Webhook misconfiguration] → Missing secret is skipped; malformed or unreachable configured endpoints fail the notification job visibly.
- [Discord rate limiting or outage] → `curl` retries transient delivery failures with a short bound; CI remains unaffected.
- [Pull-request text reaches an external service] → Send only workflow metadata needed for status, encode all values with `jq`, and do not include source, tokens, or patch content.
- [Workflow trigger availability] → `workflow_run` requires this workflow to exist on the default branch before remote notifications begin; document this deployment behavior.

## Migration Plan

1. Merge the workflow and documentation.
2. Add repository Actions secret `DISCORD_CI_WEBHOOK` containing the Discord webhook URL.
3. Confirm one completed CI run produces the expected message.
4. Roll back by deleting the notification workflow or removing the secret; existing CI validation and `gate` remain unchanged.
