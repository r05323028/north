# development-harness Specification

## Purpose

Defines North's durable engineering harness: how validation is invoked, which
hooks run when, how commits are governed, what CI must prove before merge,
and how documentation stays truthful about what is enforced versus merely
specified.

## Requirements

### Requirement: Unified validation entrypoint

The repository SHALL provide `scripts/validate.sh` with profiles fast, unit,
integration, e2e, smoke, and ci as the canonical way to run validation.
Profiles whose test layer does not exist SHALL fail explicitly instead of
pretending to pass. Docs, hooks, and CI SHALL reference this entrypoint
rather than maintaining independent command lists.

#### Scenario: Unsupported profile refuses silently passing

- **WHEN** `./scripts/validate.sh integration` runs before integration tests exist
- **THEN** the script exits non-zero with an explicit 'not yet implemented' message

#### Scenario: Fast profile exercises the whole quick loop

- **WHEN** `./scripts/validate.sh fast` runs on a clean tree
- **THEN** rustfmt check, clippy -D warnings, unit+architecture tests, web
lint/typecheck, and strict OpenSpec validation all execute and pass

#### Scenario: Unit profile covers Rust and Web units

- **WHEN** `./scripts/validate.sh unit` runs
- **THEN** it runs Rust library tests, architecture tests, and `npm test` in
`apps/web`

#### Scenario: CI profile validates Web behavior without coverage upload

- **WHEN** `./scripts/validate.sh ci` runs with `NORTH_TEST_DATABASE_URL`
- **THEN** it runs Web unit tests and Web validation without running
`npm run test:coverage` or invoking Codecov

### Requirement: Coverage upload and patch policy remain separate

CI SHALL keep Rust and Web coverage uploads in separate jobs with separate
Codecov flags. Project and flag statuses SHALL remain configured while baseline
coverage is established. Patch status MAY be temporarily disabled; when it is
re-enabled, its target SHALL be `80%` before `codecov/patch` becomes required.

#### Scenario: Temporary patch status exception

- **WHEN** Codecov evaluates this repository before baseline coverage is
established
- **THEN** Rust and Web reports upload with their existing flags and project/flag
statuses remain active
- **AND** the patch status does not block the pull request

### Requirement: Hook policy through prek

Prek SHALL manage Git hooks via `.pre-commit-config.yaml`; pre-commit SHALL run only fast hygiene and formatting. Strict OpenSpec validation SHALL use the shared entrypoint in pre-push, CI, and act parity, not as a mutating pre-commit hook. One reusable pre-push script SHALL run the native gate and act-based GitHub Actions parity with a documented escape hatch. A shared commit-message script SHALL be used by both hook and CI.

#### Scenario: Non-conventional subject is rejected at commit time

- **WHEN** a commit-msg hook receives subject "added a thing"
- **THEN** the shared validator rejects it with usage guidance

#### Scenario: Pre-push runs the same commands CI runs

- **WHEN** the pre-push hook fires
- **THEN** it executes the shared validate.sh profiles and replays a real
workflow job via act rather than a hand-maintained copy

### Requirement: Stable CI merge gate

CI SHALL end in a single required job (`gate`) that succeeds only when every
required job succeeds, so branch protection can require exactly one stable
check while job structure evolves. The repository documentation SHALL state
the exact branch-protection settings the owner must apply.

#### Scenario: Required failure blocks the gate

- **WHEN** any required job fails or is cancelled
- **THEN** the gate job fails even though other jobs succeeded

### Requirement: Truthful invariant ledger

The invariant ledger SHALL mark every invariant with a status from a small
stable vocabulary (Enforced / Partially Enforced / Specified) and SHALL NOT
cite enforcement mechanisms that do not exist.

#### Scenario: Specified invariants are not presented as enforced

- **WHEN** a reader consults the ledger for first-owner atomicity
- **THEN** they see Specified with the owning change named, not a fake test reference

### Requirement: Conditional agent tooling guidance

AGENTS.md SHALL contain conditional CodeGraph and Graphify guidance that
deactivates cleanly when `.codegraph/` or `graphify-out/` is absent, and
SHALL instruct agents to verify generated-index conclusions against current
source.

#### Scenario: No index present means no tool steps

- **WHEN** neither `.codegraph/` nor `graphify-out/` exists
- **THEN** the documented workflow contains no mandatory indexing step

### Requirement: Task completion requires evidence

A task MAY be marked complete only when the evidence demanded by that task
exists (executed validations, updated docs). Checkboxes alone SHALL NOT be
treated as proof; integration/E2E/smoke tasks SHALL NOT be completed without
execution.

#### Scenario: Unexecuted layer cannot be claimed

- **WHEN** a change's tasks include an E2E suite that was never run
- **THEN** completion claims citing it are invalid per this contract

### Requirement: Architecture checks resolve the effective dependency graph

Architecture tests SHALL evaluate dependencies via cargo metadata so normal,
dev, build, and target-scoped kinds all count, with renamed dependencies
attributed to their real crate names, and the parser SHALL carry its own
meta-test.

#### Scenario: Dev-only forbidden dependency still fails

- **WHEN** sqlx appears under [dev-dependencies] of north-domain
- **THEN** the boundary test fails

### Requirement: Production crate tree stays production-only

The repository SHALL reserve `crates/` for production Rust architectural
components. Repository-level structural validation, integration harnesses, E2E,
smoke, benchmark-only, and validation-only crates SHALL live outside `crates/`.
The architecture test package SHALL live at `tests/architecture/`, remain a
workspace member, and production crates SHALL NOT depend on it. Architecture
validation SHALL reject obvious validation-only crate names under `crates/`.

#### Scenario: Architecture tests remain outside production crates

- **WHEN** the workspace is inspected after the move
- **THEN** production packages are under `crates/`, the architecture package is
  `tests/architecture/`, and `cargo test --workspace` still executes it

#### Scenario: Accidental validation crate placement fails

- **WHEN** a crate named with `-archtests`, `-architecture-tests`,
  `-integration-tests`, `-e2e-tests`, or `-smoke-tests` is added under `crates/`
- **THEN** the architecture test fails with the repository-layout violation

### Requirement: AI-assisted documentation workflows isolate publishing credentials

The OpenWiki update workflow MUST NOT persist checkout credentials in repository configuration while model-facing tools can read repository files. The workflow MUST NOT pass its GitHub publishing token to the OpenWiki process through environment variables or files. The pull-request publishing action MUST receive its token explicitly through that action's token input.

#### Scenario: Model reads repository without checkout credential

- **WHEN** the OpenWiki model-facing process can read repository files, including `.git/config`
- **THEN** checkout credentials are not persisted in `.git/config` and the OpenWiki process receives no GitHub publishing token through its environment

#### Scenario: Pull-request action remains authenticated

- **WHEN** the workflow reaches the pull-request publishing action
- **THEN** that action receives an explicit token input with the permissions needed to create or update the workflow's pull request

### Requirement: AI-assisted documentation workflows use locked dependencies

The OpenWiki update workflow MUST install OpenWiki and its Mermaid/jsdom helper tools from committed npm package metadata and lockfile using `npm ci`. It MUST NOT use a global or other unlocked npm install. The workflow MUST invoke the repository-local OpenWiki binary so execution uses the locked installation.

#### Scenario: Workflow installs and runs locked tools

- **WHEN** the workflow installs tools and runs OpenWiki
- **THEN** it uses `npm ci --prefix .github/openwiki-tools` with the committed lockfile, then invokes `.github/openwiki-tools/node_modules/.bin/openwiki` from the repository root
