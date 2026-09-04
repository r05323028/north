# Tasks

## 1. PR-Agent workflow

- [x] 1.1 Add `.github/workflows/pr-agent.yml` using `pull_request_target`, bot-event filtering, concurrency cancellation, least-privilege permissions, and no checkout step.
- [x] 1.2 Pin `the-pr-agent/pr-agent` to the verified `v0.44.0` commit SHA and wire OpenCode Go's key through `OPENAI_KEY` plus `GITHUB_TOKEN`, with automatic review enabled and source mutation/description automation disabled.

## 2. Repository guidance and administration

- [x] 2.1 Add root `.pr_agent.toml` with OpenCode Go model routing and North-specific review priorities: invariants, architecture boundaries, security, data loss, correctness, and regression coverage.
- [x] 2.2 Document `OPENCODE_API_KEY`, OpenCode Go routing, fork-safe target-workflow rationale, advisory-only status, and rollback in `docs/development/ci.md` without changing `gate` requirements.

## 3. Validation

- [x] 3.1 Validate YAML/config shape, immutable action pin, permissions, trigger types, secret wiring, absence of checkout, and unchanged merge-gate job dependencies with focused repository checks.
- [x] 3.2 Run `openspec validate --all --strict`, relevant documentation/CI checks, and final diagnostics; review the complete diff and record any checks unavailable locally.

## Validation Notes

- `openspec validate --all --strict`: 27 passed, 0 failed.
- `./scripts/pre-push-validation.sh`: passed with ephemeral PostgreSQL; native `ci` and `act` Rust parity both completed.
- Focused Ruby YAML, Python TOML, OpenCode Go model-catalog, action-pin, permission, secret-wiring, no-checkout, merge-gate, and whitespace checks: passed.
- OpenCode Go `/zen/go/v1/models`: HTTP 200; 35 models listed, including `muse-spark-1.3-contributor`.
- `yamllint`: passed. `actionlint` and `zizmor`: unavailable locally.
- Final pi-lens diagnostics: no issues.
