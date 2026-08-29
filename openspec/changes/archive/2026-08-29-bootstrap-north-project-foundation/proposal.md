# Bootstrap North project foundation

## Why

North's agents need a repository that makes correct changes easy and incorrect
ones hard. Without an initialized workspace, enforced crate boundaries, CI, and
progressive-disclosure docs, every subsequent change rediscovers conventions
and drifts across architectural lines.

## What Changes

- Rust workspace with five boundary crates plus a structural architecture-test
  crate (`tests/architecture`) enforcing forbidden dependency edges.
- Pure domain seed encoding 0.1.0 invariants: lifecycle transition table,
  operation-specific source states, revision-bound readiness (`mark_ready` +
  stale refusal), no-op-edit semantics, edit-demotion of Ready→Discussing,
  review-packet projection, role/assignment rules. Ownership model: the
  foundation owns this seed; product changes COMPLETE and PERSIST it
  (storage/API/UI) rather than re-inventing it.
- Next.js app (`apps/web`) with App Router, TypeScript strict mode, Tailwind
  CSS v4, shadcn/ui scaffolding (components.json, `cn` util, Button), ESLint
  flat config, typecheck/build scripts.
- Documentation harness: `AGENTS.md` map, `docs/{product,architecture,
  development}` canonical docs, invariant ledger with enforcement pointers.
- GitHub Actions CI running Rust fmt/clippy/tests/archtests, web lint/
  typecheck/build, and OpenSpec validation.
- Versioned SQL migrations directory reserved with naming convention.

## Capabilities

### New Capabilities

- `project-foundation`: workspace layout, boundary enforcement, validation
  gates, and documentation harness that all later changes rely on.

### Modified Capabilities

(none)

## Impact

- Creates every top-level directory used by later changes; the pure domain seed
  is executable invariant scaffolding, not a persisted/user-facing product
  workflow. Product changes complete its storage, API, and UI ownership.
- Establishes CI contract later changes MUST keep green:
  `cargo fmt/clippy/test`, `npm lint/typecheck/build`, `openspec validate`.
- Affected docs: all of `docs/` (initial authoring), `AGENTS.md`, `README.md`.
- Dependencies on earlier changes: none — this is the root.
