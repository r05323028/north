# Documentation workflow

Progressive disclosure: `AGENTS.md` is a map; canonical truths live in focused docs;
OpenSpec carries changes until they land.

## Where things go

| Kind of statement | Home |
| --- | --- |
| Durable product semantics (lifecycle, readiness, roles) | `docs/product/*` |
| Durable architecture truths (boundaries, transport, persistence) | `docs/architecture/*` |
| Invariant ledger (what must always hold, how enforced) | `docs/development/invariants.md` |
| Change proposals/deltas/tasks | `openspec/changes/<name>/` |
| Accepted long-term behavior after archive | promoted into `docs/` |

## Rules

- Never duplicate a specification across files — link instead.
- When a change lands, update the canonical doc(s) listed in its proposal
  (“affected docs”) in the same PR.
- Do not contradict `openspec/` specs from `docs/`; if they diverge, one of them
  is wrong — fix it immediately.
- Keep `AGENTS.md` navigational; grow the deep docs, not the map.
