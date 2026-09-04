# Design

## Context

North's CI workflows are pinned to immutable action revisions, use explicit job permissions, and treat `gate` as the only merge requirement. PR-Agent will be a separate advisory workflow; it must not run North code or alter the existing validation gate.

## Goals / Non-Goals

**Goals:**

- Review pull requests, including pull requests from forks, and publish feedback through GitHub's pull-request surfaces.
- Keep workflow permissions and executed code minimal.
- Give reviews North-specific architecture and invariant context.
- Make setup reproducible and document the administrator-only secret step.

**Non-Goals:**

- Making AI review required for merge.
- Checking out or executing pull-request code.
- Adding a runtime dependency or changing North's product behavior.
- Supporting multiple model providers in this change.

## Decisions

1. **Use the upstream GitHub Action on `pull_request_target`.** This lets fork pull requests use the repository's configured secret. The workflow runs the action without checkout, so untrusted pull-request code is never executed by this job. A normal `pull_request` workflow would be safer for secret isolation but cannot provide configured-provider reviews for forks.

2. **Pin PR-Agent to release `v0.44.0` by commit SHA.** Existing North workflows do not use mutable action refs. Pinning avoids silently changing third-party code; updates are an explicit maintenance change.

3. **Grant only `contents: read`, `issues: write`, and `pull-requests: write`.** PR-Agent needs repository/PR reads and comment/review publication. It does not receive contents write or unrelated repository permissions.

4. **Use OpenCode Go through its OpenAI-compatible endpoint.** Configure PR-Agent's LiteLLM OpenAI route as `openai/muse-spark-1.3-contributor` with `https://opencode.ai/zen/go/v1`, and map the administrator's `OPENCODE_API_KEY` secret to the action's `OPENAI_KEY` input. This reuses PR-Agent's documented OpenAI-compatible configuration without committing credentials.

5. **Enable automatic review only.** Description generation and code improvement are disabled to keep output advisory and avoid automated source mutations. Review triggers mirror the workflow event types.

6. **Keep reviewer guidance in `.pr_agent.toml`.** Configuration stays versioned and reviewable, while provider credentials remain in repository settings. Guidance tells PR-Agent to prioritize North's invariants, architecture boundaries, security, data loss, and regression coverage.

## Risks / Trade-offs

- **Secret-bearing target workflow** → no checkout, immutable action pin, bot-event guard, and least-privilege token limit the blast radius; review the pinned action before upgrades.
- **Prompt injection in pull-request text** → treat PR content as untrusted data; configuration requests findings, not execution, and the job has no write access to source contents.
- **Provider outage or missing secret** → PR-Agent remains advisory and `gate` is unchanged; administrators can add/revoke `OPENCODE_API_KEY` or disable this workflow.
- **Third-party action drift after release updates** → update the SHA deliberately and rerun workflow validation.

## Migration Plan

1. Merge workflow, reviewer configuration, and documentation.
2. Add `OPENCODE_API_KEY` in repository Settings → Secrets and variables → Actions; obtain it from OpenCode Go/Zen.
3. Open or update a test pull request and confirm PR-Agent publishes review feedback without changing `gate`.
4. Roll back by disabling/removing `.github/workflows/pr-agent.yml` and revoking the secret; existing CI is unaffected.
