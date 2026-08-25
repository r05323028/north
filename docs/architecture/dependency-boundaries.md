# Dependency boundaries

Boundaries are enforced by `crates/north-archtests` (`cargo test`), which resolves Cargo metadata across normal/dev/build/target dependency kinds; do not bypass.
When a boundary changes, update the rule table AND the test together.

## Allowed dependency direction

```text
north-server ──▶ north-domain, north-protocol, north-persistence
north-daemon ──▶ north-protocol
north-persistence ──▶ north-domain (row↔domain mapping only)
north-protocol ──▶ (serde-style wire deps only)
north-domain ──▶ nothing (pure)
apps/web ──▶ server HTTP/SSE API only
```

## Forbidden edges (mechanically enforced)

| Crate | Must not depend on | Why |
| --- | --- | --- |
| north-domain | axum, tokio, sqlx, reqwest, any other north crate | pure business logic |
| north-protocol | north-domain, hosts | wire types carry no business behavior |
| north-server | north-daemon | server reaches daemon only via protocol |
| north-daemon | sqlx, persistence, domain, server | reports facts/events; no business rules or storage |
| north-persistence | axum, hosts | infrastructure serves, never drives |
| apps/web | any WebSocket client | browser↔server is HTTP + SSE only |

The frontend WebSocket ban is a structural test (`browser_never_opens_websockets`).
The frontend must also not own requirement lifecycle logic — lifecycle decisions
come from the server API; review-level convention for now, structural check when
the surface exists.

## Adding a new boundary

1. Justify it against the founding brief (invalid edges hard to introduce).
2. Prefer Cargo crate edges over lint frameworks.
3. Add the rule to `crates/north-archtests/tests/architecture.rs` and this table.
