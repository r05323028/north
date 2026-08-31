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
| Ready valid only for exact assessed content revision | Enforced | `mark_ready` revision gates + transactional assessment tests |
| Content edits bump revision and state_version once; no-op edits bump neither | Enforced | `apply_edit` canonical comparison + unit/integration tests |
| Editing Ready demotes to Discussing and advances both tokens | Enforced | `apply_edit` + integration tests |
| Review packet binds Requirement revision/state version and assessment identity | Enforced | `ReviewPacket::project`, locked packet query, stale-review integration test; generation equality is scoped to Ready reviewability |
| Accept/Reject/Request Changes/Reopen human-only, reviewer-gated | Enforced | `Role::can_review` + server guard + assessment-bound transition integration tests |
| Requirement access is workspace-wide in 0.1.0; no per-Requirement ACL | Enforced | authenticated routes and cross-requester integration test |
| First account atomically Owner; later accounts Requester | Enforced | transactional `AuthStore::verify_code` owner claim + concurrency test |
| Verification codes cannot be brute-forced past a bounded attempt budget | Enforced | locked transactional failed-attempt counter, five-failure consumption, PostgreSQL concurrency test |
| Active OTP values resist database-only offline recovery | Specified | keyed OTP hashing deferred to `harden-otp-at-rest`; current SHA-256 remains documented debt |
| Conversation is context, not source of truth | Enforced | migration 0004, requester/paginated APIs, structured-edit route, and PostgreSQL integration tests |
| Existing-Requirement mutations require expected_state_version; revision remains content-only | Enforced | locked persistence operations, HTTP 409 handlers, and integration tests |
| `requirement.assessed` evidence, transition, dedupe, and ACK share one commit boundary | Enforced | typed server conversion, migration 0005, event-id dedupe, post-commit ACK service, and PostgreSQL integration tests |

## Architecture & runtime

| Invariant | Status | Enforcement |
| --- | --- | --- |
| Forbidden crate dependency edges absent (normal/dev/build/target kinds) | Enforced | archtests via effective cargo metadata graph |
| No dumping-ground crates | Enforced | archtests directory scan |
| Browser never opens WebSockets | Enforced | `browser_never_opens_websockets` structural test |
| Browser SSE is notification; reconnect/refocus/hints refetch canonical API state | Partially Enforced | authenticated `/events`, post-commit identity hints, web unit/Playwright coverage; full server-backed Board/detail E2E pending |
| Daemon reports facts/events; server owns business transitions | Partially Enforced | crate edges now; server-side transition validation lands with requirement-domain change |
| Server is sole owner of durable business state | Partially Enforced | dependency boundaries; server/persistence implementation and integration tests pending |
| Setup approval state changes require authenticated same-origin POST | Enforced | read-only approval GET, Origin/Host validation, and PostgreSQL HTTP-boundary tests |
| Public auth/setup request endpoints have resource-aware abuse limits | Specified | deferred to `harden-public-endpoint-abuse-protection`; no current limiter claimed |
| Every server command is durable before dispatch and idempotent at daemon boundary | Enforced | immutable outbox transaction, payload digest, daemon Journal, stable command/runtime identity, and duplicate suppression |
| Command ACK means durable daemon acceptance, not runtime completion | Enforced | daemon `received` journal commit precedes `command_ack`; runtime outcome is separate |
| Command/event ids and independent directional sequences detect gaps and harmless duplicates | Enforced | server watermarks/event ledger plus bounded daemon Journal identity and sequence checks |
| Protocol 0.1.x rejects incompatible/unknown frames deterministically | Enforced | codec validation, bidirectional terminal `protocol.error`, transport decode handling, and coordinator conflict handling |
| Active session is durably pinned to one daemon; no automatic live migration | Enforced | migrations 0007–0009, `AuthStore::start_session_with_command`, requirement-bound session context, `DaemonRuntime::persist_and_dispatch_command`, and reconciliation resend |
| Multi-server connection ownership epochs are enforced | Specified | single-server 0.1.0 only; HA ownership epochs deferred |
| Durable command redelivery and ACK replay survive process failure | Enforced | server outbox/watermarks plus daemon Journal recovery, replay, and non-expiring identity tombstones |
| Single-server restart invalidates stale daemon connection leases | Enforced | `build_app` clears `connected_at`/`connection_id` before serving; restart/reconnect integration coverage |
| Expired daemon setup rows have bounded retention | Partially Enforced | indexed 24-hour retention and 100-row opportunistic cleanup on setup create/poll; no scheduler in 0.1.0 |
| Setup claim response is retry-idempotent after a lost response | Specified | accepted 0.1.0 one-shot claim trade-off; no plaintext credential recovery |
| Daemon credentials are user-owned; Admin/Owner revocation cuts access | Enforced | migration 0007, device-flow claim, authenticated WS registration, owner/admin revoke routes, per-frame connection revalidation, and PostgreSQL integration coverage |
| Server owns execution state, retry budget, attempt count, and terminal Failed | Specified | introduce-runtime-retry-and-failure-state + hardening; restart/retry tests pending |
| Daemon has no business retry policy authority | Partially Enforced | architecture source guard; runtime implementation must keep policy in server |
| Execution failure never mutates Requirement lifecycle state | Specified | runtime retry change; isolation integration test pending |

## Repository access

| Invariant | Status | Enforcement |
| --- | --- | --- |
| Clarification never intentionally persists mutations to source repos | Partially Enforced | daemon uses disposable clone, read-only Git allowlist, post-task dirty check, and cleanup integration tests; process-level detection/response, NOT sandbox-enforced |
| Concurrent sessions never share a mutable inspection checkout | Enforced | unique session/task/repository workspace allocation, per-repository cache lock, and concurrent host-Git integration test |
| Git credentials never centralized in the server | Enforced | repository/protocol DTOs contain metadata only; host-Git environment is inherited by daemon and credential-bearing locations are rejected |
| Configured repositories are soft-disabled, not normally hard-deleted | Enforced | migration 0013, Admin/Owner lifecycle routes, idempotent timestamps, and retained identity |
| Disabled repositories are excluded from new inspections | Enforced | enabled-only active catalog plus mandatory immutable run repository authorization; retained in-flight IDs remain valid after disable |
| Inspections cite exact commit SHAs | Enforced | detached checkout verifies Git-resolved SHA; readiness accepts only complete SHA-1/SHA-256 widths; moving-ref integration test |

## Persistence & retention

| Invariant | Status | Enforcement |
| --- | --- | --- |
| Ephemeral runtime data never sole source of truth; TTL GC touches ephemeral tables only | Specified | pending introduce-runtime-event-retention |
| Durable vs ephemeral class split | Specified | docs/architecture/persistence.md; retention implementation pending |
| Durable command/event sequence watermarks survive daemon restart and safe command compaction | Enforced | server execution-session watermarks and daemon Journal persisted high-water/tombstone state |

## Transport and protocol

| Invariant | Status | Enforcement |
| --- | --- | --- |
| `north-protocol` is independent from Axum/Tokio/Tungstenite and host crates | Enforced | Cargo metadata rules in `tests/architecture`; pure JSON codec tests |
| Server daemon transport is Axum WebSocket + JSON text; daemon transport is tokio-tungstenite | Enforced | adapter modules, dependency metadata, and real Axum↔tokio-tungstenite integration tests |
| WebSocket ping/pong does not replace North heartbeat | Enforced | adapter tests, authenticated heartbeat persistence, 45-second stale-status expiry, and protocol/docs |
| Transport errors stay distinct from North protocol errors | Enforced | `TransportError`/`ConnectionError` variants and adapter tests |
| `session.start` carries server-assembled requirement, bounded conversation, and enabled repository metadata | Partially Enforced | `north-server::assemble_session_start` + unit test; persistence/session coordinator pending |
| `requirement.assessed` carries typed verdict/evidence, not opaque assessment text | Enforced | `north-protocol` validation/round-trip tests, explicit server/domain conversion, immutable evidence persistence, and post-commit ACK handling |
| Daemon application traffic waits for welcome, reconciliation, and coordination readiness | Enforced | explicit supervisor phases plus real transport gating integration test |
| Protocol/auth failures stop daemon reconnect | Enforced | terminal failure classification, bidirectional protocol errors, authenticated/revoked handling, and durable coordinator failure boundaries |
| `north-domain` and `north-protocol` obey positive dependency allowlists | Enforced | Cargo metadata allowlist tests |
| Connection reconciliation is one validated snapshot delivered to coordination before Active | Enforced | typed snapshot, canonical sparse ACK validation, handshake result, Journal merge/replay, readiness gate, and activation integration tests |
| Axum/tokio-tungstenite do not provide North reliability | Enforced | transport adapters plus server registration, bounded liveness, immutable outbox, Journal, ACKs, reconciliation, and bounded recovery |
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
