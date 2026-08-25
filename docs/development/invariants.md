# Invariant ledger

Status vocabulary (stable):

- **Enforced** — a running mechanism fails when violated today (test, domain
  API, structural check).
- **Partially Enforced** — enforced within stated limits; limits named.
- **Specified** — target architecture documented in OpenSpec/docs; NOT yet
  executable. Do not cite it as current truth.

When you add an invariant: extend this ledger AND add enforcement, or mark it
Specified with the owning change named. Documentation alone is not enforcement.

## Domain

| Invariant | Status | Enforcement |
| --- | --- | --- |
| Callers cannot bypass Requirement business rules to mutate state | Enforced | private invariant-bearing fields; mutation only via operations; unit tests |
| Lifecycle operations pin their source state (reopen only from Rejected, etc.) | Enforced | domain ops + unit tests |
| Ready valid only for exact assessed revision | Enforced | `mark_ready` gates + unit tests |
| Content-changing edits bump revision once; no-op edits change nothing | Enforced | `apply_edit` canonical comparison + unit tests |
| Editing Ready demotes to Discussing (stale invalidation) | Enforced | `apply_edit` + unit tests |
| Review packet is projection(Requirement, revision-matched assessment); stale packet unreviewable | Enforced | `ReviewPacket::project` + unit tests |
| Accept/Reject/Request Changes/Reopen human-only, reviewer-gated | Specified | pending introduce-role-and-permission-model (domain helpers seeded) |
| First account atomically Owner; later accounts Requester | Specified | pending introduce-email-auth-and-owner-bootstrap |
| Conversation is context, not source of truth | Specified | product doc; enforceable once persistence lands |

## Architecture & runtime

| Invariant | Status | Enforcement |
| --- | --- | --- |
| Forbidden crate dependency edges absent (normal/dev/build/target kinds) | Enforced | archtests via effective cargo metadata graph |
| No dumping-ground crates | Enforced | archtests directory scan |
| Browser never opens WebSockets | Enforced | `browser_never_opens_websockets` structural test |
| Daemon reports facts/events; server owns business transitions | Partially Enforced | crate edges now; server-side transition validation lands with requirement-domain change |
| Stable ids on every command/event; at-least-once + idempotent processing; server ACKs processed event ids | Specified | pending introduce-server-daemon-protocol |
| Daemon-initiated connection; heartbeat liveness | Specified | pending introduce-daemon-runtime-connection |
| Execution failure ≠ requirement failure; Failed only after retry budget exhausted | Specified | pending introduce-runtime-retry-and-failure-state |

## Repository access

| Invariant | Status | Enforcement |
| --- | --- | --- |
| Clarification never intentionally persists mutations to source repos | Partially Enforced | specified mechanism: disposable checkout + dirty-tree violation detection (introduce-local-repository-inspection); process-level, NOT sandbox-enforced |
| Git credentials never centralized in the server | Specified | schemas will carry no credential fields; daemon uses host git env |
| Inspections cite exact commit SHAs | Specified | pending introduce-local-repository-inspection |

## Persistence & retention

| Invariant | Status | Enforcement |
| --- | --- | --- |
| Ephemeral runtime data never sole source of truth; TTL GC touches ephemeral tables only | Specified | pending introduce-runtime-event-retention |
| Durable vs ephemeral class split | Specified | docs/architecture/persistence.md |
