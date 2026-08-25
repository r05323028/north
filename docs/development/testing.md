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
protocol serialization round-trips; daemon workspace + host git.

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
layer boundaries, repository layout, and transport restrictions; they do not
prove product behavior.

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
| Integration | Not implemented — arrives with introduce-email-auth-and-owner-bootstrap (real DB) |
| E2E | Not implemented — arrives with UI surface (introduce-requirement-board establishes pattern) |
| Smoke | Not implemented — arrives with runnable server/web artifacts |

## Structural coverage

| Surface | Status |
| --- | --- |
| Architecture | Implemented — `cargo test -p north-architecture-tests` (effective cargo metadata graph, dumping grounds, layout, frontend WebSocket ban) |

Unsupported profiles exit explicitly (`validate.sh` exit 3) rather than
pretending to pass.

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
