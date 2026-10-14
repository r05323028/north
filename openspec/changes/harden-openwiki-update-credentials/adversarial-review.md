# Adversarial Review

## Assumptions Under Attack

- Threat model is prompt-injected use of OpenWiki's supported repository file reader, not arbitrary runner compromise. Current source confines shell commands to `pwd` and `git rev-parse HEAD` (`/Users/seanchang/.local/share/mise/installs/node/26.7.0/lib/node_modules/openwiki/dist/agent/docs-only-backend.js:361-384`); `/Users/seanchang/.local/share/mise/installs/node/26.7.0/lib/node_modules/openwiki/dist/agent/docs-only-backend.js:177-184` can read repository files.
- Current workflow has one checkout (`.github/workflows/openwiki-update.yml:17-22`), no other Git auth/config setup, and OpenWiki env contains model/LangSmith secrets but no GitHub token (`.github/workflows/openwiki-update.yml:33-47`). PR action follows model step (`.github/workflows/openwiki-update.yml:65-70`).

## Findings

No credential-exposure findings remain in declared scope. Advisor review agreed that disabling checkout persistence closes the demonstrated `.git/config` exposure if the later PR action receives its token explicitly.

The committed manifest/lockfile and `npm ci` remove the `adhoc-packages` finding; `zizmor .github/workflows/openwiki-update.yml` now reports no findings. `npm audit` still reports seven advisories in the existing pins: five high in the OpenWiki/deepagents dependency chain (no fix available), one moderate in `mermaid@11.16.0`, and one low in KaTeX. Mermaid `11.17.2` addresses the latter two; the user chose to retain existing versions and report these as upstream risk. Locking improves repeatability, not vulnerability status.

Accepted limitation, not an open warning: same-job execution is not isolation against arbitrary runner/action/code compromise. If that enters scope, isolate model execution in a read-only job and publish in a separate write-permission job. Current change targets the evidenced file-read route; design states this boundary.

## Missing Scenarios

Regression tests must scope checks to named steps: exactly one checkout with `persist-credentials: false` before OpenWiki; no token env/Git credential setup before the model; explicit token input only on the later PR action. Also verify committed package metadata and lockfile, `npm ci`, and use of the local OpenWiki binary.

## Unresolved Questions

The GitHub-hosted workflow runtime has not been executed; local linters and architecture tests verify static configuration only.

## Verdict

Static checks confirm credential isolation, locked installation, and a clean `zizmor` run. `npm audit` remains non-clean by user decision; GitHub-hosted workflow runtime was not executed. Existing dependency advisories are not fixed by this change.
