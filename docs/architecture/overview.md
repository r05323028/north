# Architecture overview

```text
┌────────────┐  HTTP + SSE   ┌────────────────┐  persistent conn  ┌────────────────┐
│  Next.js   │ ────────────▶ │  Rust server   │ ◀──────────────── │   Rust daemon  │
│  apps/web  │ ◀──────────── |  north-server  │ ────────────────▶ |  north-daemon  │
└────────────┘   (browser)   └───────┬────────┘  (daemon dials)   └───────┬────────┘
                                     │                                    │
                              relational DB                     local filesystem, host git,
                          (north-persistence)                  agent runtime (local machine)
```

- The browser talks **only** to the server (HTTP commands, SSE live updates).
- The daemon always initiates the connection (NAT/firewall friendly).
- The server owns all business state; the daemon reports facts/events.

## Crates

| Crate | Responsibility |
| --- | --- |
| north-domain | requirements, lifecycle, readiness, roles — pure logic |
| north-server | HTTP/SSE API, auth/sessions, business transitions, daemon connection endpoint |
| north-daemon | local git workspaces, agent runtime, event reporting, reconnect |
| north-protocol | command/event envelopes shared by server and daemon |
| north-persistence | SQL storage behind repository traits |
| north-archtests | structural tests enforcing dependency boundaries |

## Transports

| Edge | Transport |
| --- | --- |
| Browser → Server | HTTP |
| Server → Browser (live) | SSE |
| Server ↔ Daemon | daemon-initiated WebSocket (TLS in deployment) |

UI stack: Next.js App Router, TypeScript, Tailwind CSS, shadcn/ui components.

Details: dependency-boundaries.md, server-daemon-protocol.md, daemon.md,
repository-access.md, persistence.md.
