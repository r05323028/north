# Architecture overview

```text
┌────────────┐  HTTP + SSE   ┌────────────────┐  persistent conn  ┌────────────────┐
│  Next.js   │ ────────────▶ │  Rust server   │ ◀──────────────── │   Rust daemon  │
│  apps/web  │ ◀──────────── |  north-server  │ ────────────────▶ |  north-daemon  │
└────────────┘   (browser)   └───────┬────────┘  (daemon dials)   └───────┬────────┘
                                     │                                    │
                              relational DB                     local cache, disposable
                          (north-persistence)                  checkouts, host git,
                                                               runtime, transport journal
```

- Browser talks **only** to server over HTTP and SSE. SSE is notification; after
  connect/reconnect UI refetches canonical API state.
- Daemon always initiates persistent WebSocket connection.
- Server owns durable business state, session ownership, command outbox, and
  execution retry policy. Daemon reports execution facts and owns only local
  transport/runtime recovery.
- Server↔daemon command/event delivery is at-least-once. Stable ids prevent
  duplicate effects; independent per-session sequence spaces detect gaps.

## Crates

| Crate | Responsibility |
| --- | --- |
| north-domain | requirements, lifecycle, readiness, roles — pure logic |
| north-server | HTTP/SSE API, auth, sessions, business transitions, command outbox, daemon routing, execution policy |
| north-daemon | daemon-initiated connection, durable transport journal, runtime coordination, fact/event reporting; production agent runtime and repository checkouts remain downstream |
| north-protocol | command/event/control envelopes and compatibility metadata only |
| north-persistence | SQL storage and transactional row↔domain mapping |

The daemon's local transport journal is not North business state and is not
server database access. It records enough command/event delivery state to make
reconnects idempotent.

## Repository validation

Structural architecture validation lives outside production `crates/`, under
`tests/architecture/`. It enforces dependency direction, repository layout,
transport restrictions, and daemon retry-policy ownership; it does not prove
runtime delivery, database concurrency, workspace isolation, or SSE behavior.

| Validation surface | Responsibility |
| --- | --- |
| `tests/architecture/` | structural dependency, layout, transport, and ownership checks |
| integration tests | protocol journals, outbox/ACK/replay, persistence transactions, daemon workspaces |
| E2E tests | browser reconnect/refetch and user-visible lifecycle behavior |

## Transport boundary

```text
Browser ── HTTP + SSE ──▶ north-server
                             │
                             └── Axum WebSocket transport
                                      │ JSON text frames
                                      ▼
                               north-protocol
                                      ▲
                                      │
                          tokio-tungstenite transport
                                      │
                                  north-daemon
```

| Edge | Transport | Owner |
| --- | --- | --- |
| Browser → Server | HTTP | Axum/server routes |
| Server → Browser (live) | SSE notification hints; canonical state comes from HTTP | north-server |
| Server ↔ Daemon | WebSocket over TLS in deployment | Axum on server; tokio-tungstenite on daemon |
| WebSocket payload | JSON text, one North frame per WebSocket text message | north-protocol schema |

Axum and tokio-tungstenite provide WebSocket upgrade, framing, ping/pong,
close, socket I/O, limits, and TLS integration. `north-protocol` defines only
the application wire contract. North coordination owns IDs, ordering,
at-least-once delivery, outbox/journals, ACKs, reconciliation, idempotency,
and recovery; WebSocket ping/pong never replaces North heartbeat.

Before `session.start`, server coordination assembles the requirement snapshot,
bounded conversation excerpt, and enabled repository metadata. The daemon sees
only these North DTOs and has no database or credential access. Daemon connection
phases are `Connecting → AwaitingWelcome → Authenticated → Reconciling →
ReconciliationReceived → Active`; coordination applies the connection-level
snapshot before readiness, and normal application traffic begins only at `Active`.

UI stack: Next.js App Router, TypeScript, Tailwind CSS, shadcn/ui components.

Details: dependency-boundaries.md, server-daemon-protocol.md, daemon.md,
repository-access.md, persistence.md.
