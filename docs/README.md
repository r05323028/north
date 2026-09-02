# North documentation

Read top-down; each doc stays small and links instead of duplicating.

- [DESIGN.md](DESIGN.md) — product UI design system extracted from current prototype

## Product (semantics agents must not contradict)

- [product/requirement-lifecycle.md](product/requirement-lifecycle.md) — states, transitions, who owns which move
- [product/readiness.md](product/readiness.md) — Ready gates, revision binding, stale invalidation
- [product/roles-and-permissions.md](product/roles-and-permissions.md) — four roles, review rights, bootstrap rules
- [product/conversation.md](product/conversation.md) — conversation vs. structured truth

## Architecture (how the system holds together)

- [architecture/overview.md](architecture/overview.md) — topology, crates, transports
- [architecture/dependency-boundaries.md](architecture/dependency-boundaries.md) — forbidden edges + enforcement
- [architecture/server-daemon-protocol.md](architecture/server-daemon-protocol.md) — envelope contract, delivery semantics
- [architecture/daemon.md](architecture/daemon.md) — responsibilities, retry posture
- [architecture/repository-access.md](architecture/repository-access.md) — read-only local-git philosophy
- [architecture/persistence.md](architecture/persistence.md) — durable vs ephemeral, migrations, owner bootstrap

## Development (working rules)

- [development/invariants.md](development/invariants.md) — invariant ledger + where each is enforced
- [development/testing.md](development/testing.md) — normative layers, coverage, profiles
- [development/ci.md](development/ci.md) — workflow jobs, stable gate, act parity
- [development/git-workflow.md](development/git-workflow.md) — PRs and Conventional Commits
- [development/tooling.md](development/tooling.md) — prek, act, CodeGraph, Graphify
- [development/documentation.md](development/documentation.md) — where statements belong; update duties

Change proposals and deltas live in `openspec/` (see `AGENTS.md`).
