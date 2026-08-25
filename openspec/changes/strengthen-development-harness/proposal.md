# Strengthen development harness

## Why

Foundation review found correctness gaps (public mutation paths bypassing
domain invariants, edge-vs-operation confusion, no-op edit churn, packet
source-of-truth drift, protocol direction ambiguity, an overclaimed read-only
guarantee) and harness gaps (no unified validation entrypoint, no hooks, no
commit policy enforcement, CI without a stable gate, an invariant ledger that
mixed enforced with merely-specified). Fixing these BEFORE the next product
change keeps every later change cheap to verify.

## What Changes

- Domain: private invariant-bearing Requirement fields + read-only accessors;
  operation-specific source states; no-op edits change nothing; ReviewPacket
  as a projection of Requirement + revision-matched assessment.
- Protocol docs/types: three disjoint frame groups (control / commands /
  events); explicit server→daemon acknowledgement; resume reconciliation via
  control frames; session.resume stays command-only.
- Repository access: honest read-only statement (disposable checkout +
  dirty-tree violation detection; process-level, not sandbox-enforced).
- Architecture tests resolve the EFFECTIVE cargo dependency graph
  (normal/dev/build/target kinds, renamed deps) via cargo metadata, with a
  parser meta-test.
- Harness: scripts/validate.sh profiles (fast/unit/integration/e2e/smoke/ci),
  prek hooks (hygiene, fmt, commit-msg Conventional Commit validation,
  pre-push full gate incl. act CI parity), CI pr-title job + stable `gate`
  job, truthful invariant-ledger statuses, CodeGraph/Graphify conditional
  guidance, task-completion-evidence rule, development docs restructure.
- OpenSpec ownership model: foundation owns the domain seed; product changes
  complete/persist it (wording aligned across existing changes).

Out of scope: any 0.1.0 product behavior change; Bazel/Nx/task-runners; fake
integration/E2E/smoke suites; kernel-enforced sandboxes.

## Capabilities

### New Capabilities

- `development-harness`: validation entrypoint contract, hook policy, commit
  rules, CI gate contract, ledger truthfulness, agent guidance conditionality,
  completion-evidence rule, effective-graph architecture checking.

### Modified Capabilities

- `requirements` (spec of introduce-requirement-domain-model): encapsulation,
  operation-source-state, no-op-edit semantics now stated as seed-owned
  invariants completed by that change; wording corrected.
- `readiness` (spec of introduce-readiness-assessment): review packet is a
  projection, not assessment-owned truth.
- `daemon-protocol` (spec of introduce-server-daemon-protocol): control-frame
  group added; acknowledgement contract made explicit; direction ambiguity removed.
- `repository-inspection` (spec of introduce-local-repository-inspection):
  read-only guarantee restated at its true enforcement level.
- `roles`, `email-auth`, `conversations`, `project-foundation`: minor wording
  alignment with the seed-ownership model.

## Impact

- Affected code: crates/north-domain (encapsulation + ops + packet),
  crates/north-archtests (metadata-based checks), crates/north-protocol (docs),
  scripts/*, .pre-commit-config.yaml, .github/workflows/ci.yml, AGENTS.md,
  docs/development/*.
- New local tool expectations: prek, act (+Docker) — both optional-with-
  documentation when absent.
- Dependencies on earlier changes: none beyond existing foundation.
