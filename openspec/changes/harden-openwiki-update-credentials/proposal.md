# Proposal

## Why

The scheduled OpenWiki workflow runs model-driven repository inspection. The OpenWiki file reader can read `.git/config`, while `actions/checkout` persists its token there by default. A prompt-injected model can therefore expose a repository-write credential. This is a concrete credential-boundary gap, not a claim that exploitation occurred.

## What Changes

Require the model-facing OpenWiki step to run without checkout credentials persisted in the repository. Require the pull-request action to receive its publishing token explicitly in that action step. Install OpenWiki and its Mermaid/jsdom helpers from a committed npm lockfile via `npm ci`, then invoke the workspace-local OpenWiki binary. Add regression assertions and document both boundaries.

Credential isolation and the locked dependency graph are invariants. `persist-credentials: false`, an explicit PR-action token, `npm ci`, and a local binary are implementation choices.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `development-harness`: constrain credential handling in AI-assisted documentation workflows.

## Dependencies

No North application runtime dependencies change. Dedicated workflow-only npm metadata locks OpenWiki and its helpers; existing CI and release workflows remain outside this change.

## Impact

- `.github/workflows/openwiki-update.yml`
- `.github/openwiki-tools/package.json` and `.github/openwiki-tools/package-lock.json`
- `tests/architecture/tests/architecture.rs`
- `docs/development/ci.md` (canonical documentation)
- `openspec/specs/development-harness/spec.md` when change lands
