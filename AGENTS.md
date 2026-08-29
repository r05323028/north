# AGENTS.md — working on North

North is a self-hosted requirement-management system: requesters
collaborate with an AI agent to turn ambiguous requests into structured,
reviewable requirements. Boring and explicit beats clever. Invariants get
mechanisms, not prose.

## CodeGraph / Graphify (conditional tools)

- If `.codegraph/` exists at the repo root, prefer
  MCP `codegraph_explore` when connected; use shell `codegraph explore
  "<question>"` when that CLI command exists. This checkout's CLI uses
  `codegraph context "<question>"` and `codegraph query "<symbol>"`. Prefer
  these over broad grep/find when locating or understanding code. Verify
  exact behavior against current source before correctness claims. Missing `.codegraph/`
  → skip; never generate an index unprompted.
- If `graphify-out/` exists: read `graphify-out/GRAPH_REPORT.md` before
  broad architecture exploration; use `graphify query/path/explain` for
  targeted questions; after changes run `graphify update .` when
  configured. Treat `graphify-out/` as generated data — never hand-edit
  it. Missing `graphify-out/` → skip. Details:
  docs/development/tooling.md.

## Navigation map

| Topic | Canonical location |
| --- | --- |
| Product semantics | docs/product/ |
| Architecture & boundaries | docs/architecture/ |
| Testing layers & coverage truth | docs/development/testing.md |
| CI jobs, merge gate, act parity | docs/development/ci.md |
| Branches, PRs, Conventional Commits | docs/development/git-workflow.md |
| prek, act, CodeGraph, Graphify | docs/development/tooling.md |
| Invariant ledger with honest statuses | docs/development/invariants.md |
| Documentation rules | docs/development/documentation.md |
| Change management | openspec/ |

## OpenSpec (mandatory for behavior changes)

All behavioral/engineering-contract changes go through OpenSpec:

```bash
openspec new change <kebab-name>   # proposal → specs → design → tasks
openspec validate --all --strict   # must pass before finishing
```

Update the canonical doc named in a change's proposal when the change
lands. Do not invent competing specification systems.

## Validation (single entrypoint)

```bash
./scripts/validate.sh fast   # fmt·clippy·unit+archtests·web lint/tc·specs
./scripts/validate.sh ci     # full workspace gate + web build + specs
./scripts/pre-push-validation.sh  # ci gate + act parity vs real workflow jobs
```

### Pre-push validation decision

Before deciding whether to run the pre-push hook, inspect the actual Git
changed-file set. Include staged and unstaged tracked changes plus untracked
files (for example, the union of `git diff --name-only HEAD` and
`git ls-files --others --exclude-standard`). Include pre-existing changes; do
not decide from task intent or commit-message text.

A change qualifies as documentation-only when, and only when, every changed
file matches this allowlist:

- `*.md` files;
- documentation-content files under `docs/**`; and
- documentation-site content files, such as Markdown/MDX pages, that contain
  only documentation content and cannot affect site build or runtime
  configuration.

Documentation-site configuration is not allowlisted, even when adjacent to
content. Unknown or ambiguous files default to non-documentation-only.

Decision rule:

```text
all changed files are docs-only allowlisted files
    -> skip ./scripts/pre-push-validation.sh
any changed file is outside the docs-only allowlist
    -> run ./scripts/pre-push-validation.sh
```

Any source, test, manifest or lockfile, build/tool/formatter/linter/compiler
configuration, CI workflow, script, container file, generated code, schema or
fixture consumed by executable code, documentation-site configuration, or
mixed documentation plus non-documentation diff requires the normal pre-push
validation. Skipping the hook for a strictly allowlisted documentation diff
does not remove other relevant documentation or specification checks.

Unsupported test profiles exit explicitly — never fake a suite for a
name. Any command documented here must actually work.

## Test layers (summary)

Unit · Integration · E2E · Smoke — normative definitions and current
coverage: docs/development/testing.md. One layer per test; execution
environment is not a layer.

## Pull requests / commits

- `main` receives changes via PRs; required check is the **`gate`** job.
- Squash merging: the PR title is the canonical commit subject on `main`
  and MUST be a Conventional Commit
  (`feat|fix|docs|test|refactor|perf|build|ci|chore|revert`).
- Branch commits may stay informal; local commit-msg hook validates by
  default. Details: docs/development/git-workflow.md.

## Invariants you may not break

Ledger with honest status per invariant:
docs/development/invariants.md. Headlines (see ledger for enforcement):

- Daemon reports facts/events; the server owns business state.
- Browser ↔ daemon direct communication never happens (HTTP + SSE only).
- Server ↔ daemon uses Axum WebSocket + North JSON text protocol +
  tokio-tungstenite; transport libraries provide transport only, while North
  coordination owns reliability, idempotency, replay, ordering, and recovery.
- Server assembles complete `session.start` context; daemon enters Active only
  after hello/welcome, one reconciliation snapshot, and coordination readiness.
  Retryable socket failures back off; protocol/auth failures stop reconnect.
  `command_ack` and `event_ack`
  are canonical ACK names.
- Ready is valid only for the exact revision assessed; edits demote
  Ready → Discussing.
- Requirement state mutates only through domain operations.
- Clarification never intentionally persists mutations to sources.
- `crates/` is reserved for production Rust architectural components.
- Repository-level validation belongs under `tests/` or the appropriate tooling
  surface; do not add architecture, integration, E2E, smoke, benchmark-only, or
  validation-only crates under `crates/`.
- Do not weaken or bypass `tests/architecture`; extend it when adding boundaries.
  Canonical layout rule: `docs/architecture/dependency-boundaries.md`.

## Task completion rules

A task is complete only when its evidence exists. OpenSpec checkboxes are
progress records, not proof. Before declaring done: review the diff, run
the relevant validate.sh profiles, run architecture checks, update
affected canonical docs, run strict OpenSpec validation, and keep the
gate green. Never mark integration/E2E/smoke tasks complete if they
were not executed.
