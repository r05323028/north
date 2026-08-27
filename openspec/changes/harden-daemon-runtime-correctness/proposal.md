## Why

Verification of `introduce-daemon-runtime-connection` found correctness and security gaps at state-changing HTTP, authentication-code, command persistence, setup-request lifecycle, restart, and CLI retry boundaries. Fix them now before runtime connection behavior becomes a compatibility contract, while preserving North 0.1.0's single-server, server-owned architecture.

## What Changes

- Make setup approval read-only on `GET` and authenticated, origin-checked, state-changing on `POST`; expose daemon label in confirmation state.
- Bound failed verification attempts transactionally while retaining six-digit codes, expiry, cooldown, and supersession semantics.
- Construct one typed command envelope, persist its exact serialized representation, then dispatch that persisted command; keep raw dispatch internal.
- Opportunistically delete setup requests expired beyond a retention window with bounded indexed cleanup.
- Invalidate persisted daemon connection leases at single-server startup so reconnect is required after restart.
- Make daemon setup polling retry connection failures and retryable 5xx responses with bounded backoff, while stopping on terminal errors and expiry.
- Add focused unit, HTTP-boundary, persistence, concurrency, and PostgreSQL integration coverage.

Out of scope: Redis or scheduler infrastructure, durable redelivery/replay, HA ownership epochs, replacing the CLI `curl` subprocess unless the existing change is trivial, and unrelated refactors.

## Capabilities

### New Capabilities

- `daemon-runtime`: Secure setup approval, exact durable command dispatch, bounded setup-request retention, restart-safe connection leases, and resilient setup polling.

### Modified Capabilities

- `email-auth`: Verification codes gain a bounded transactional failed-attempt budget in addition to existing expiry, single-use, cooldown, and supersession behavior.

## Impact

Affected areas: `crates/north-server` setup/auth/daemon handlers and runtime state, `crates/north-persistence` auth and daemon SQL operations, `crates/north-daemon` setup polling, migrations, PostgreSQL integration tests, CI/test documentation, and OpenSpec canonical specs. Existing HTTP routes remain compatible except that setup approval `GET` no longer mutates state and invalid/unauthenticated approval POSTs are rejected.
