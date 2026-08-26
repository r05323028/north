# Design

## Context

Repository rows are referenced by readiness evidence. A hard delete would make
old evidence dependent on a missing catalog row, while a reusable clone used as
a runtime working tree lets concurrent clarification runs contaminate one
another. The cross-cutting repository contract is canonical in
`harden-distributed-system-architecture`.

## Decisions

- `repositories` table: UUID id, unique name, URL, description, timestamps,
  and nullable `disabled_at`; no credential/token/key/password fields.
- Admin/Owner Remove sets `disabled_at`; active list and session catalog filter
  it out. Keep the row and metadata for historical joins. Do not hard-delete a
  referenced row in 0.1.0.
- Catalog endpoint for daemon/session context returns enabled metadata only;
  inspection start rechecks enabled status server-side.
- Repository credentials stay on the daemon host and are used through normal
  Git environment; server persistence never sees them.
- Inspection runtime uses a per-repository reusable cache only as source
  material. Each session/task creates a unique disposable plain checkout, so
  concurrent sessions never share a mutable directory. Dirty checkout means
  discard and report; no Git worktree dependency.
- Inspection result remains `{repository_id, commit_sha, notes}`; the full SHA
  and retained repository row make evidence reproducible and human-readable.

## Risks / Trade-offs

- **Catalog metadata can become stale** → inspection validates enabled status
  at start and records exact SHA.
- **Soft-disabled rows accumulate** → they are durable history by design; any
  future purge requires an explicit evidence snapshot/migration, not silent
  deletion.
- **Process-level checkout isolation is not a sandbox** → dirty detection and
  disposal are required; stronger OS isolation remains a future threat-model
  decision.
