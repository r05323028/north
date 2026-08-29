## Purpose

Establishes the enforceable skeleton every North change builds on: monorepo
layout, crate boundary rules that make invalid dependencies unbuildable, a
domain seed carrying the core 0.1.0 invariants, and validation gates (Rust,
web, specs) that CI runs on every commit.

## ADDED Requirements

### Requirement: Workspace builds and validates cleanly

The repository SHALL provide a Cargo workspace whose members build without
warnings under `-D warnings`, pass `cargo fmt --check`, and pass
`cargo test --workspace`; and a Next.js app that passes lint, typecheck, and
production build.

#### Scenario: Fresh clone passes the full gate

- **WHEN** CI runs fmt, clippy (`--workspace --all-targets -- -D warnings`),
and test on the workspace, then lint, typecheck, and build in `apps/web`
- **THEN** every command exits successfully

### Requirement: Dependency boundaries are mechanically enforced

The system SHALL fail `cargo test` whenever a crate declares a forbidden
dependency edge (domain→infra/hosts, protocol→domain/hosts, server→daemon,
daemon→persistence/domain/server, persistence→hosts), whenever a dumping-ground
crate (`common`/`shared`/`utils`/`helpers`/`core`) appears, or whenever the
browser bundle opens a WebSocket.

#### Scenario: Forbidden edge fails the build

- **WHEN** a developer adds `sqlx` to `crates/north-domain/Cargo.toml`
- **THEN** `cargo test --workspace` fails with an explanatory violation message

#### Scenario: Browser transport stays HTTP/SSE-only

- **WHEN** frontend source contains `new WebSocket`, `ws://`, or `wss://`
- **THEN** the structural test `browser_never_opens_websockets` fails

### Requirement: Domain encodes lifecycle and readiness invariants

The domain crate SHALL refuse illegal lifecycle transitions, SHALL accept
`mark_ready` only when the assessment targets the current revision with a Ready
verdict, no blockers, and existing acceptance criteria, SHALL bump the revision
and demote Ready→Discussing on any accepted edit, and SHALL restrict review
decisions and role assignment per the roles matrix.

#### Scenario: Stale assessment cannot make a requirement Ready

- **WHEN** `mark_ready` receives an assessment bound to an older revision
- **THEN** the call fails with a stale-assessment error and state is unchanged

#### Scenario: Editing a Ready requirement demotes it

- **WHEN** an accepted edit applies to a Ready requirement
- **THEN** the revision increments by one and status becomes Discussing

### Requirement: Progressive-disclosure documentation harness

The repository SHALL ship `AGENTS.md` as a navigational map pointing to
`docs/product`, `docs/architecture`, `docs/development`, and `openspec/`, with
an invariant ledger listing each invariant and where it is enforced.

#### Scenario: Agent can find canonical truth in two hops

- **WHEN** an agent reads `AGENTS.md` and follows its links
- **THEN** it reaches the authoritative doc for any lifecycle, readiness,
role, boundary, transport, or persistence question without duplication

### Requirement: CI mirrors local validation

CI SHALL run the exact commands documented in `docs/development/testing.md`
for Rust, web, and OpenSpec on pushes and pull requests.

#### Scenario: Spec drift is caught

- **WHEN** an OpenSpec change fails `openspec validate --all --strict`
- **THEN** the CI openspec job fails
