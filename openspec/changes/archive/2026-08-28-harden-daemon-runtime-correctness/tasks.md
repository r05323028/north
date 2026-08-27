## 1. Persistence schema and transactional state

- [x] 1.1 Add forward migration for verification failed-attempt state and indexed daemon setup expiry; expose bounded constants.
- [x] 1.2 Update verification issuance/consumption so failed attempts commit under row lock, reaching the limit consumes the code, and new codes reset/supersede prior codes; add unit/database concurrency coverage.
- [x] 1.3 Add read-only setup preview, bounded expired-request cleanup, and startup daemon-connection lease invalidation persistence operations; add PostgreSQL coverage for state transitions and cleanup.
- [x] 1.4 Run targeted `north-persistence` tests and migration-backed verification checks.

## 2. Server approval, startup, and command delivery

- [x] 2.1 Split setup approval GET/POST behavior, return daemon label in read-only preview, enforce authenticated same-origin POST approval, and add HTTP-boundary CSRF/invalid-token coverage.
- [x] 2.2 Run lease invalidation after migrations before serving, propagate startup failures, and add restart placement coverage while preserving connection-ID race and clean-disconnect behavior.
- [x] 2.3 Replace public arbitrary-payload/raw-frame pairing with typed command construction that serializes once, persists exact payload before dispatch, and leaves persisted data unchanged on dispatch failure; extend integration assertions.
- [x] 2.4 Run targeted server unit and PostgreSQL daemon integration tests.

## 3. Daemon setup polling

- [x] 3.1 Classify curl transport/status failures, retry connection errors and 5xx responses with bounded backoff, stop on terminal errors/expiry, and add focused polling classification tests.
- [x] 3.2 Run targeted daemon tests and CLI checks.

## 4. Documentation and completion validation

- [x] 4.1 Update canonical daemon/auth/testing/invariant documentation with CSRF, attempt limits, cleanup, restart lease, and polling behavior; record HA and durable redelivery as deferred.
- [x] 4.2 Run format, clippy, unit, architecture, web, PostgreSQL integration, merge-gate, and strict OpenSpec validation; review diff and preserve all existing runtime invariants.

## 5. Browser approval flow and explicit 0.1.0 debt

- [x] 5.1 Return escaped HTML confirmation UI for browser approval GET, retain JSON read-only preview for explicit API clients, and return human-readable HTML after browser POST while preserving authentication and same-origin checks.
- [x] 5.2 Add HTTP/integration coverage for browser GET → label/state/form → non-mutating GET → same-origin POST → CLI-style claim, including JSON negotiation, secret exclusion, conflict, invalid, expired, unauthenticated, and cross-origin cases.
- [x] 5.3 Document browser GET/POST/claim semantics and explicitly record public endpoint abuse protection, keyed OTP hashing, one-shot claim recovery, HA ownership epochs, and durable redelivery/ACK processing as deferred.
- [x] 5.4 Create or follow separate OpenSpec follow-ups for public endpoint abuse protection and keyed OTP hashing without changing those mechanisms in this branch.
- [x] 5.5 Run focused and full repository validation, strict OpenSpec validation, and review the final change boundary before archive.
