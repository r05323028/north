## Purpose

Makes North's current architectural boundaries mechanically visible while leaving distributed behavior to integration tests that run after the owning components exist.

## ADDED Requirements

### Requirement: Structural dependency and transport boundaries remain enforced

Architecture tests SHALL reject forbidden dependency edges across all Cargo
dependency kinds, reject browser WebSocket clients, and preserve the fixed
North topology: protocol has no business/host dependencies, daemon has no
persistence/domain/server dependencies, server has no daemon dependency, and
persistence has no host dependency.

#### Scenario: Boundary regression fails before runtime implementation

- **WHEN** a future change adds a forbidden crate edge or frontend WebSocket marker
- **THEN** the architecture test fails with the owning boundary named

### Requirement: Daemon cannot become retry-policy owner

Architecture validation SHALL reject daemon-only declarations of the
server-owned execution state or business retry budget (for example an
`ExecutionState`/`RetryPolicy` owner or `MAX_ATTEMPTS` authority) while allowing
local WebSocket backoff and runtime transport recovery mechanics. Server-owned
attempt state remains a design/implementation contract until north-server exists.

#### Scenario: Business retry logic does not move into daemon

- **WHEN** daemon source declares a server execution state or business retry-budget type/constant
- **THEN** architecture validation fails and points to server ownership

### Requirement: Behavioral guarantees have implementation-time proofs

OpenSpec implementation tasks for durable command delivery, sequence gap
reconciliation, expected-revision conflicts, atomic assessment ACKs, daemon
pinning/revocation, workspace isolation, repository disablement, and SSE
reconnect SHALL include runnable integration/E2E tests before those tasks are
marked complete. Documentation and architecture tests SHALL NOT be reported as
proof of these runtime guarantees.

#### Scenario: Spec-only claim is not marked enforced

- **WHEN** the runtime mechanism for a distributed invariant does not yet exist
- **THEN** the invariant ledger labels it Specified or Partially Enforced and names the owning implementation/test task
