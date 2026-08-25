# AGENTS.md — working on North

North is a self-hosted requirement-management system: requesters collaborate with an AI
agent to turn ambiguous requests into structured, reviewable requirements.

## Start here (progressive disclosure)

1. Product semantics you must not contradict: `docs/product/`
   (requirement lifecycle, readiness, roles, conversation).
2. System shape and hard boundaries: `docs/architecture/`
   (overview, dependency boundaries, protocol, daemon, repository access, persistence).
3. Workflow, invariant ledger, testing, documentation duties: `docs/development/`.

## OpenSpec (mandatory for behavior changes)

All behavioral changes go through OpenSpec (`openspec/`):

```bash
openspec new change <kebab-name>     # scaffold
# fill artifacts: proposal → specs → design → tasks
openspec validate --change <name> --strict
```

Implement against the task list, keep validation green, and update the canonical
doc in `docs/` when the change lands (see docs/development/documentation.md).
Do not invent a competing specification system.

## Validation

- Rust: `cargo fmt --all --check` · `cargo clippy --workspace --all-targets -- -D warnings` · `cargo test --workspace`
- Web (`apps/web`): `npm ci` · `npm run lint` · `npm run typecheck` · `npm run build`
- Specs: `openspec validate --all --strict`

## Invariants you may not break (ledger: docs/development/invariants.md)

- Daemon reports facts/events; the **server** owns business state transitions.
- The browser never communicates directly with the daemon. Browser↔server: HTTP + SSE.
  Server↔daemon: daemon-initiated persistent connection (WebSocket).
- Requirement state survives daemon disconnects and runtime-log expiry;
  ephemeral runtime data is never the source of truth.
- `Ready` is valid only for the exact requirement revision the agent assessed;
  any edit demotes Ready → Discussing and forces re-assessment.
- Accept / Request Changes / Reject are human decisions restricted to
  Requirement Manager, Admin, Owner. Request Changes ≠ Reject.
- Repository inspection is read-only and uses the daemon host's own `git`
  and auth environment; Git credentials never travel to the server.
- Conversation history is context, not truth; the structured Requirement is canonical.
- Do not weaken or bypass `crates/north-archtests`; extend it when adding boundaries.

## House rules

- No `common/` / `utils/` / `shared/` dumping grounds; code lives beside the concept it serves.
- Prefer boring, explicit dependencies over clever abstractions.
- Distinguish **invariants** (make them enforceable) from **implementation suggestions** (leave freedom).
- Update the canonical doc when behavior changes; never duplicate a specification across files.
