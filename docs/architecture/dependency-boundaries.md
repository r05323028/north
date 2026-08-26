# Dependency boundaries

Boundaries are enforced by `tests/architecture` (`cargo test`), which resolves
Cargo metadata across normal/dev/build/target dependency kinds; do not bypass.
When a boundary changes, update the rule table AND the test together.

## Repository layout boundary

`crates/` contains production Rust components that participate in North's
runtime/application architecture. Repository-level structural validation,
architecture tests, integration harnesses, E2E tests, smoke tests, and similar
non-production verification code live outside the production crate tree.

```text
Production architecture:
apps/
crates/

Repository validation/testing:
tests/
scripts/
.github/
```

Validation crates must not live under `crates/`; keeping them outside preserves
the production dependency graph and component ownership model.

## Allowed dependency direction

```text
north-server ──▶ north-domain, north-protocol, north-persistence, Axum/Tokio host
north-daemon ──▶ north-protocol, Tokio, tokio-tungstenite host
north-persistence ──▶ north-domain (row↔domain mapping only)
north-protocol ──▶ serde/serde_json wire deps only
north-domain ──▶ nothing
apps/web ──▶ server HTTP/SSE API only
```

`north-daemon` may use local filesystem APIs for its transport journal and
isolated checkouts. That is transport/local-host recovery, not server database
access or durable Requirement state.

## Pure-crate positive allowlists

`north-domain` declares no dependencies. `north-protocol` declares only
`serde` and `serde_json`, across normal, dev, build, and target-specific Cargo
dependency kinds. Architecture tests use positive allowlists for these two
crates; the broader forbidden-edge rules below remain defense in depth.

## Forbidden edges (mechanically enforced)

| Crate | Must not depend on | Why |
| --- | --- | --- |
| north-domain | axum, tokio, sqlx, reqwest, any other north crate | pure business logic |
| north-protocol | axum, tokio, tokio-tungstenite, tungstenite, north-domain, north-server, north-daemon, north-persistence | pure North JSON wire contract; no transport/runtime or business host types |
| north-server | north-daemon | server reaches daemon only through north-protocol |
| north-daemon | axum, sqlx, north-persistence, north-domain, north-server | daemon transport may use Tokio + tokio-tungstenite; no server host or business DB/lifecycle rules |
| north-persistence | axum, north-server, north-daemon | infrastructure serves, never drives |
| apps/web | any WebSocket client | browser↔server is HTTP + SSE only |

Structural source checks also reject daemon ownership of server execution state
or business retry budgets while allowing WebSocket reconnect/backoff and local
runtime transport recovery. Once repository schemas exist, architecture tests
must reject server-side credential/token/key/password fields; until then this
remains a specified implementation obligation, not a false passing check.

The frontend WebSocket ban is a structural test
(`browser_never_opens_websockets`). The frontend must also not own Requirement
lifecycle logic — lifecycle decisions come from the server API; review-level
convention for now, structural check when the surface exists.

## Transport boundary

The server WebSocket endpoint is an Axum upgrade handler followed by a thin
transport adapter. The adapter starts the hello deadline immediately after
upgrade, reads hello before bounded coordinator admission, and forwards decoded
JSON text into `north-protocol` channels without business transitions or
persistence logic. The daemon uses one `tokio-tungstenite` connection supervisor
with a single writer, an independent reader, bounded outbound buffering, local
reconnect backoff, and an explicit handshake-result boundary for coordination.
No Socket.IO, custom WebSocket framing, or generic unused transport abstraction
is allowed.

`north-protocol` must not expose Axum, Tokio, Tungstenite, or WebSocket message
types. Browser code remains HTTP + SSE only.

## Adding a new boundary

1. Justify it against the founding brief (invalid edges hard to introduce).
2. Prefer Cargo crate edges over lint frameworks.
3. Add the rule to `tests/architecture/tests/architecture.rs` and this table.
4. Add a targeted negative test and update `docs/development/invariants.md`.
