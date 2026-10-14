# Tasks

## 1. Harden credentials and dependency installation

- [x] 1.1 Disable checkout credential persistence and give the PR action an explicit token input, retaining full history.
- [x] 1.2 Add architecture regression coverage for one checkout before OpenWiki, disabled persistence, no pre-model token/Git auth configuration, and explicit token on the later PR action.
- [x] 1.3 Update `docs/development/ci.md` and canonical `openspec/specs/development-harness/spec.md` for credential isolation and locked tool installation, preserving existing user edits.
- [x] 1.4 Add exact-version workflow-only npm manifest and committed lockfile; replace global unlocked installation with `npm ci --prefix .github/openwiki-tools` and run the local OpenWiki binary from repository root.
- [x] 1.5 Extend architecture regression coverage to require `npm ci`, the local binary, expected exact package versions, and a committed lockfile.

Validation: `npm ci --prefix .github/openwiki-tools`; `cargo test --manifest-path tests/architecture/Cargo.toml --test architecture openwiki_update_workflow_preserves_credentials_boundary`; `actionlint .github/workflows/openwiki-update.yml`; `zizmor .github/workflows/openwiki-update.yml`; `openspec validate --all --strict`; `git diff --check`.

## 2. Verify integration and preserve remaining release gates

- [x] 2.1 Re-review the complete changed diff, confirm no credential path before OpenWiki and no `adhoc-packages` finding; record accepted npm audit advisories and any unrun hosted-runner limitation.
- [x] 2.2 Run `./scripts/pre-push-validation.sh` after all edits; do not claim exact-source package/OCI/Compose/browser qualification without required owner inputs.

Validation: `./scripts/pre-push-validation.sh`; final `openspec validate --all --strict`; inspect actual changed-file set.
