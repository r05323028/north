# Proposal

## Why

North has CI and branch-protection guidance but no automated pull-request review. Adding a pinned PR-Agent workflow gives maintainers repeatable AI-assisted review feedback without changing North runtime or product behavior.

## What Changes

- Add a GitHub Actions workflow that runs PR-Agent on pull-request activity.
- Use least-privilege comment/read permissions, an immutable PR-Agent action revision, and no checkout of untrusted pull-request code.
- Add repository review instructions focused on North invariants, architecture boundaries, security, data loss, and missing tests.
- Configure OpenCode Go as the model provider through its OpenAI-compatible endpoint.
- Document the required `OPENCODE_API_KEY` repository secret and fork-safe workflow behavior.

Out of scope: changing North runtime code, CI merge-gate semantics, branch protection, or adding PR-Agent as a required status check.

## Capabilities

### New Capabilities

None. This is CI tooling and documentation only; `.openspec.yaml` sets `skip_specs: true`.

### Modified Capabilities

None.

## Impact

- Affected automation: `.github/workflows/pr-agent.yml`.
- Affected reviewer configuration: `.pr_agent.toml`.
- Affected canonical documentation: `docs/development/ci.md`.
- Required repository administration: add `OPENCODE_API_KEY` under GitHub repository Actions secrets.
- Model routing: `openai/muse-spark-1.3-contributor` via `https://opencode.ai/zen/go/v1`.
- No production APIs, protocol messages, persistence, or runtime dependencies change.
