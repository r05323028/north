# Documentation workflow

Progressive disclosure: `AGENTS.md` is a map; canonical truths live in focused docs;
OpenSpec carries changes until they land.

## Audience boundary

`docs/` is North's structured, agent-oriented knowledge base and remains canonical for
product, architecture, and development facts. `web/` is the independently authored,
human-facing manual for users and contributors. Site pages may summarize canonical
facts, but link to their source instead of creating competing specifications; the
site does not import or generate pages from `docs/`.

## Where things go

| Kind of statement | Home |
| --- | --- |
| Durable product semantics (lifecycle, readiness, roles) | `docs/product/*` |
| Durable architecture truths (boundaries, transport, persistence) | `docs/architecture/*` |
| Invariant ledger (what must always hold, how enforced) | `docs/development/invariants.md` |
| Change proposals/deltas/tasks | `openspec/changes/<name>/` |
| Repository layout and validation placement | `docs/architecture/dependency-boundaries.md` |
| Accepted long-term behavior after archive | promoted into `docs/` |
| Human user guides, contributor guidance, release-history entry point | `web/src/content/docs/*` |

## Rules

- Never duplicate a specification across files — link instead.
- When a change lands, update the canonical doc(s) listed in its proposal
  (“affected docs”) in the same PR; update affected human guides when user or
  contributor workflows change.
- Human guides may restate a workflow briefly, but product and architecture
  semantics stay canonical in `docs/`; link to exact sources.
- Changelog entries represent published releases; do not treat OpenSpec proposals
  or unreleased work as release history.
- Do not contradict `openspec/` specs from `docs/`; if they diverge, one of them
  is wrong — fix it immediately.
- Keep `AGENTS.md` navigational; grow the deep docs, not the map.
