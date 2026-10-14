# Design

## Context

OpenWiki needs full history for documentation diffs and can read `.git/config`. Checkout's default credential persistence exposes its token to that reader. The workflow also globally installs OpenWiki and helpers without a committed lockfile; `zizmor` flags this as `adhoc-packages`.

## Goals / Non-Goals

**Goals:** retain full-history updates and authenticated PR creation without giving the model checkout credentials; install deterministic OpenWiki tooling from a committed lockfile.

**Non-Goals:** redesign the sandbox, split jobs, defend against arbitrary runner/code compromise, change provider secrets, permissions, PR policy, or release qualification. North application runtime dependencies remain unchanged.

## Decisions

- Set checkout `persist-credentials: false`; keep `fetch-depth: 0`. Checkout can fetch, but leaves no credential for later repository reads.
- Give `peter-evans/create-pull-request` an explicit `token: ${{ secrets.GITHUB_TOKEN }}` input. Do not pass token values to OpenWiki env/files. Keep current job permissions; this closes confirmed `.git/config` exposure, not every possible runner/action compromise.
- Add `.github/openwiki-tools/package.json` and committed `.github/openwiki-tools/package-lock.json` with exact versions for OpenWiki, Mermaid, and jsdom. Use `npm ci --prefix .github/openwiki-tools`; invoke `.github/openwiki-tools/node_modules/.bin/openwiki` while keeping repository root as the working directory. This removes global unlocked installs and preserves OpenWiki's repo context.
- Add architecture assertions for named-step credential ordering, locked package metadata, `npm ci`, and the local binary. Document both boundaries in `docs/development/ci.md`.

## Risks / Trade-offs

Future authenticated Git commands need their own explicit credential. Dependency upgrades require changing and reviewing both manifest and lockfile. Locking does not remediate known npm advisories; existing versions remain by user decision, as recorded in the adversarial review. Current OpenWiki update does not push; PR action publishes. Static assertions verify configuration, not hosted-runner semantics.
