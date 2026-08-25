# Design

## Context

The repository starts empty apart from the OpenSpec skeleton. Agents implementing
0.1.0 changes need enforced boundaries from day one, or the first feature lands
business logic in convenient-but-wrong places.

## Decisions

- **Structural tests over frameworks**: dependency rules live in
  `tests/architecture` as plain Rust tests that parse manifests and frontend
  sources. No arch-lint DSL until rules outgrow grep-level checks (they are six
  crates today).
- **Dependency-free stubs**: host crates (`north-server`, `north-daemon`,
  `north-persistence`, `north-protocol`) carry zero dependencies until their
  owning change introduces them. Fast builds, trivially auditable boundaries.
- **Domain seed**: lifecycle transitions, revision-bound readiness, and role
  rules exist as pure code with unit tests so later changes extend instead of
  reinvent them.
- **shadcn/ui over a component zoo**: components are generated into
  `components/ui` via the shadcn CLI and owned in-tree.
- **npm** as package manager (lockfile committed); ESLint flat config directly
  consuming `eslint-config-next`'s native flat array (no FlatCompat shim).

## Open Questions

None blocking. Database engine choice (Postgres vs SQLite-first) is deferred to
`introduce-email-auth-and-owner-bootstrap` where the first migration lands.
