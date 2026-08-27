# Testing

Run everything before finishing a change; CI mirrors these commands.
Canonical entrypoint: **`./scripts/validate.sh <profile>`** — do not hand-
assemble command lists in docs/hooks/CI; they all call these scripts.

## Test layers (normative definitions)

Every significant test has exactly ONE primary functional layer. Execution
environment (Docker, CI, local) is a separate concept from layer.

### Unit

One small unit of behavior with minimal external dependencies. Fast,
deterministic, isolated. Examples: requirement lifecycle, readiness gates,
role assignment, protocol value helpers, pure frontend functions where
appropriate.

### Integration

Real interaction across meaningful component boundaries, preferring real
infrastructure over mocks that re-implement behavior. Examples: north-server +
north-persistence against a real test DB; HTTP endpoint → persistence;
protocol serialization/replay; daemon journal + host git + disposable checkout.

### E2E

A user-observable workflow across the assembled system (sign in → create
requirement → clarify → Ready → review → accept). Proves system behavior, not
internal functions. Playwright for browser workflows once that surface exists.
Never added as placeholders just to justify the layer name.

### Smoke

The built/deployed system basically starts and serves essential surfaces:
server boots, migrations apply, health endpoint responds, web starts and can
reach the server. Shallow and fast by design.

Classification rules: do not promote integration tests to E2E because Docker
is involved; do not call a test smoke merely because it is quick.

## Structural validation

Architecture tests are repository-level structural validation, not one of the
functional behavior layers. They verify dependency direction, forbidden edges,
layer boundaries, repository layout, transport restrictions, and ownership
markers; they do not prove product behavior.

```text
Functional behavior validation
├── Unit
├── Integration
├── E2E
└── Smoke

Structural validation
└── Architecture
```

The architecture crate lives at `tests/architecture/` and remains a workspace
member so `cargo test --workspace` executes it.

## Functional coverage (truthful)

| Layer | Status |
| --- | --- |
| Unit (Rust) | Implemented — `cargo test --workspace --lib` (domain invariants) |
| Integration | Partially implemented — real Axum↔tokio-tungstenite and PostgreSQL-backed daemon lifecycle coverage; durable coordination proofs remain pending |
| E2E | Not implemented — arrives with UI surface changes |
| Smoke | Not implemented — arrives with runnable server/web artifacts |

## Required future proofs

| Contract | Primary layer | Owning change |
| --- | --- | --- |
| exact command envelope persistence/order, daemon inbox, duplicate `message.send`, restart recovery | Integration | harden-daemon-runtime-correctness covers exact persistence/order; durable inbox/replay remains introduce-server-daemon-protocol + daemon connection |
| sequence gaps, late/out-of-order replay, protocol errors | Integration | introduce-server-daemon-protocol |
| expected_revision HTTP 409 and no side effects | Integration | requirement-domain/conversations/human-review |
| atomic assessment evidence/transition/dedupe before event ACK | Integration | readiness-assessment |
| daemon selection, pinned reconnect, credential revocation | Integration | daemon-runtime-connection |
| server retry authority and restart-persistent attempts | Integration | runtime-retry-and-failure-state |
| concurrent disposable checkouts, dirty discard, exact SHA | Integration | local-repository-inspection |
| soft-disable history and disabled-repo rejection | Integration | configured-repositories |
| SSE disconnect/missed hint/duplicate hint refetch | E2E | requirement-board + requirement-conversation-ui |

Documentation, OpenSpec checkboxes, and architecture tests do not prove these
runtime guarantees. Do not mark their implementation tasks complete until the
runnable test exists and passes.

## Profiles

```bash
./scripts/validate.sh fast   # fmt, clippy, unit + architecture, web lint/typecheck, openspec
./scripts/validate.sh unit   # unit + architecture
./scripts/validate.sh ci     # full workspace gate + web build + specs
./scripts/validate.sh integration | e2e | smoke   # explicit 'not yet' until real
```

## Web (apps/web)

Components come from shadcn/ui (`npx shadcn@latest add <component>`); do not
fork them casually. Frontend unit tests arrive with the board change.

## Specs

`openspec validate --all --strict` runs inside `fast`, `ci`, and pre-push.

## Server↔daemon transport checks

Unit tests cover every `north-protocol` frame family, JSON text round trips,
setup approval Origin/Host policy, bounded verification-attempt accounting,
setup-row retention, restart lease invalidation, and exact outbox persistence,
assembled `session.start` context, typed readiness evidence, canonical
`command_ack`/`event_ack` serialization, execution-only `session.resume`,
unsupported schema/unknown frame rejection, Axum text-frame conversion, binary
frame rejection, transport ping/pong handling, handshake phases/timeouts,
terminal protocol classification, bounded queues, admission deadlines, reconciliation activation, and reconnect backoff reset.
Architecture tests mechanically confirm pure-crate allowlists, both hosts'
`north-protocol` dependency, server/daemon separation, and the browser
WebSocket ban. The real transport integration test is
`tests/transport/tests/websocket.rs` and runs with
`cargo test -p north-transport-integration --test websocket`. The PostgreSQL-backed
daemon lifecycle test runs locally with `NORTH_TEST_DATABASE_URL` and is required
in CI job `daemon-integration`. Durable outbox redelivery/ACK processing, journal
replay, authentication persistence, socket reconnect sequencing, durable
reconciliation restore, and browser SSE behavior remain integration/E2E
obligations; exact persistence-before-dispatch and restart lease behavior are
covered by the PostgreSQL daemon suite.
