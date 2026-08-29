## 1. Domain correctness

- [x] 1.1 Private invariant-bearing Requirement fields + read-only accessors (crates/north-domain/src/requirement.rs)
- [x] 1.2 Operation-specific source states; Draft.reopen / Discussing.reopen / Rejected.begin_discussion / Discussing.accept|reject refusal tests
- [x] 1.3 No-op edit semantics: empty/same-value edits keep revision+status; actual edits bump once; Ready demotion only on real change (tests)
- [x] 1.4 ReviewPacket projection refusing stale pairs (readiness.rs + tests)
- Evidence: cargo test --workspace --lib = 19 passed; negative fixture (sqlx in domain manifest) fails archtests, reverted.

## 2. Architecture enforcement

- [x] 2.1 archtests parse cargo metadata --no-deps; all dependency kinds count; renamed deps attributed correctly
- [x] 2.2 Parser meta-test covering dev/build/target/rename cases
- Evidence: cargo test -p north-architecture-tests = 6 passed including meta-test and layout/dependency guards.

## 3. Protocol & repo-access truth

- [x] 3.1 north-protocol lib.rs + docs/architecture/server-daemon-protocol.md: control/command/event groups, server ACK, resume-as-command-only
- [x] 3.2 docs/architecture/repository-access.md: honest read-only section (disposable checkout + dirty-tree violation; process-level)
- [x] 3.3 introduce-server-daemon-protocol + introduce-local-repository-inspection specs updated to match

## 4. Harness

- [x] 4.1 scripts/validate.sh (fast/unit/integration/e2e/smoke/ci; unsupported exit 3; unit/ci include Web Vitest)
- [x] 4.2 scripts/pre-push-validation.sh (native gate + web build + act job replay, escape hatch)
- [x] 4.3 scripts/check-commit-message.sh (--self-test covers accept/reject matrix)
- [x] 4.4 .pre-commit-config.yaml: hygiene + rustfmt (pre-commit); commit-msg validator; pre-push entrypoint; strict OpenSpec validation stays in shared gates
- [x] 4.5 ci.yml: pr-title job, split rust/web/coverage/openspec jobs, stable gate job, temporary Codecov patch status exception
- [x] 4.6 docs/development/{testing,ci,git-workflow,tooling,invariants}.md rewritten/new; AGENTS.md navigation map with conditional CodeGraph/Graphify + completion rules
- Evidence: `./scripts/validate.sh unit` and `ci` run Rust, architecture, and Web unit validation; integration/e2e/smoke exit 3 with explicit message; check-commit-message --self-test 8/8; coverage uploads remain separate while temporary patch status is disabled; ci.yml gate logic reviewed against needs.*.results.

## 5. OpenSpec consistency

- [x] 5.1 Seed-ownership model documented; bootstrap + product-change proposals worded accordingly
- [x] 5.2 introduce-readiness-assessment packet wording fixed to projection semantics
- [x] 5.3 openspec validate --all --strict green at task close (rerun before archive)
- Evidence: 17/17 changes pass strict validation; act openspec job also passes.

## 6. Repository layout invariant

- [x] 6.1 Move architecture validation crate to `tests/architecture/`; rename
  package to `north-architecture-tests`; keep it in the Cargo workspace.
- [x] 6.2 Add mechanical guard for validation-only crate names under `crates/`
  and forbid production dependencies on the architecture-test package.
- [x] 6.3 Update canonical architecture/testing docs, AGENTS.md, README.md,
  OpenSpec context, and active change references.
- Evidence: 6 architecture tests pass; layout and dependency negative fixtures fail as expected; `validate.sh fast/ci`, strict OpenSpec (17/17), prek, and act pre-push pass; stale-reference sweep is empty.

## 7. Owner actions (outside repo)

- [ ] 7.1 Apply branch protection on main requiring the `gate` check (steps: docs/development/ci.md)
