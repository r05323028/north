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
| Conversation is context, not source of truth | Specified | pending introduce-requirement-conversations; persistence/API test required |
| Every existing-Requirement mutation requires expected_revision | Specified | harden-distributed-system-architecture; server/persistence CAS + HTTP 409 test pending |
| `requirement.assessed` evidence, transition, dedupe, and ACK share one commit boundary | Specified | harden-distributed-system-architecture; readiness integration test pending |

## Architecture & runtime

| Invariant | Status | Enforcement |
| --- | --- | --- |
| Forbidden crate dependency edges absent (normal/dev/build/target kinds) | Enforced | archtests via effective cargo metadata graph |
| No dumping-ground crates | Enforced | archtests directory scan |
| Browser never opens WebSockets | Enforced | `browser_never_opens_websockets` structural test |
| Browser SSE is notification; reconnect refetches canonical API state | Specified | harden-distributed-system-architecture; board/detail E2E pending |
| Daemon reports facts/events; server owns business transitions | Partially Enforced | crate edges now; server-side transition validation lands with requirement-domain change |
| Server is sole owner of durable business state | Partially Enforced | dependency boundaries; server/persistence implementation and integration tests pending |
| Every server command is durable before dispatch and idempotent at daemon boundary | Specified | harden-distributed-system-architecture; protocol integration test pending |
| Command ACK means durable daemon acceptance, not runtime completion | Specified | harden-distributed-system-architecture; protocol integration test pending |
| Command/event ids and independent directional sequences detect gaps and harmless duplicates | Specified | harden-distributed-system-architecture; protocol replay/gap tests pending |
| Protocol 0.1.x rejects incompatible/unknown frames deterministically | Specified | harden-distributed-system-architecture; protocol compatibility tests pending |
| Active session is durably pinned to one daemon; no automatic live migration | Specified | harden-distributed-system-architecture; session routing/reconnect tests pending |
| Daemon credentials are user-owned; Admin/Owner revocation cuts access | Specified | introduce-daemon-runtime-connection + hardening; connection/revocation tests pending |
| Server owns execution state, retry budget, attempt count, and terminal Failed | Specified | introduce-runtime-retry-and-failure-state + hardening; restart/retry tests pending |
| Daemon has no business retry policy authority | Partially Enforced | architecture source guard; runtime implementation must keep policy in server |
| Execution failure never mutates Requirement lifecycle state | Specified | runtime retry change; isolation integration test pending |

## Repository access

| Invariant | Status | Enforcement |
| --- | --- | --- |
| Clarification never intentionally persists mutations to source repos | Specified | hardening + local-inspection contract; disposable-workspace integration test pending; process-level, NOT sandbox-enforced |
| Concurrent sessions never share a mutable inspection checkout | Specified | hardening/local-inspection tasks; concurrent workspace test pending |
| Git credentials never centralized in the server | Specified | repository schema task must omit credential fields; architecture schema check when schema exists |
| Configured repositories are soft-disabled, not normally hard-deleted | Specified | configured-repositories task; migration/API/history test pending |
| Disabled repositories are excluded from new inspections | Specified | configured-repositories/local-inspection tasks; integration test pending |
| Inspections cite exact commit SHAs | Specified | pending introduce-local-repository-inspection |

## Persistence & retention

| Invariant | Status | Enforcement |
| --- | --- | --- |
| Ephemeral runtime data never sole source of truth; TTL GC touches ephemeral tables only | Specified | pending introduce-runtime-event-retention |
| Durable vs ephemeral class split | Specified | docs/architecture/persistence.md; retention implementation pending |
| Durable command/event sequence watermarks survive daemon restart and safe command compaction | Specified | hardening; daemon journal/restart integration test pending |

## Transport and protocol

| Invariant | Status | Enforcement |
| --- | --- | --- |
| `north-protocol` is independent from Axum/Tokio/Tungstenite and host crates | Enforced | Cargo metadata rules in `tests/architecture`; pure JSON codec tests |
| Server daemon transport is Axum WebSocket + JSON text; daemon transport is tokio-tungstenite | Enforced | adapter modules and dependency metadata; full endpoint/runtime integration pending |
| WebSocket ping/pong does not replace North heartbeat | Partially Enforced | adapter tests and protocol/docs; authenticated liveness persistence pending |
| Transport errors stay distinct from North protocol errors | Enforced | `TransportError`/`ConnectionError` variants and adapter tests |
| Axum/tokio-tungstenite do not provide North reliability | Specified | server-daemon protocol contract; outbox/journal/reconciliation implementation pending |
| Browser communication remains HTTP + SSE; browser opens no WebSocket | Enforced | `browser_never_opens_websockets` architecture test |

## Existing good architecture preserved

- Requirement lifecycle remains distinct from execution state.
- Ready remains valid only for the exact assessed Requirement revision; edits
  demote Ready → Discussing.
- Human review remains server-authorized; conversation/history remains context,
  never Requirement truth.
- Raw chain-of-thought and raw runtime/tool logs remain excluded from product
  messages and subject to runtime TTL.
- Daemon initiates the server connection; browser never connects to daemon and
  never uses WebSocket.
- Repository credentials remain on daemon hosts; no object storage or external
  broker is required for 0.1.0.
