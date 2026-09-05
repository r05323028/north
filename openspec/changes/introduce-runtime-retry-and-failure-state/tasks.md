## 1. Canonical capability cleanup

- [ ] Replace the overlapping `execution-state` delta with a modification of
      `openspec/specs/execution-retry-authority`.
- [ ] Align clarification-runtime and Requirement workspace contracts with
      active `retrying` and terminal `failed` projection semantics.
- [ ] Update the daemon-protocol delta so runtime events keep identity,
      sequence, dedupe, and ACK-after-commit while reaching owning server
      projections instead of generic rejection.
- [ ] Keep session ownership/daemon-runtime as consumed boundaries; remove
      stale wording that treats retry behavior as a future duplicate capability.

## 2. Durable persistence and transaction boundaries

- [ ] Add migration fields for attempt count, snapshotted max attempts,
      `next_retry_at`, safe failure class/reason, and current attempt identity.
- [ ] Add narrow `execution_attempts` persistence with unique session/attempt,
      command, and failure-event identities; add due-retry index.
- [ ] Make initial `session.start` and every `session.resume` commit command,
      attempt row, counter update, and current-attempt identity atomically.
- [ ] Make accepted `session.failed` close/clear current attempt N before
      entering Retrying or terminal Failed; due scheduling requires no current
      attempt and makes N+1 current.
- [ ] Backfill existing sessions conservatively and document restart behavior.

## 3. Attempt identity/accounting

- [ ] Count initial start and each new resume exactly once at durable command
      creation.
- [ ] Prove reconnect, command ACK retry, daemon journal replay, event replay,
      heartbeat, reconciliation, cancel, and message commands do not count.
- [ ] Keep attempt identity run-scoped and prevent a delayed old run from
      affecting a newer run.

## 4. Failure facts and policy

- [ ] Process `session.failed` as an attempt fact after event identity/sequence
      validation and classify to bounded safe reasons.
- [ ] Implement known-failure retry/exhaustion transitions without Requirement
      mutation. Once unknown outcome terminalizes a run, prohibit all later
      `session.resume`; cover new-run/new-start recovery with current context and
      normal slot/state-version rules.
- [ ] Make duplicate/replayed failure facts return the original ACK/outcome and
      perform no second budget, schedule, resume, or terminal effect.

## 5. Durable scheduler and pinned ownership

- [ ] Implement startup plus bounded polling/wakeup discovery of due rows; do
      not make in-memory timers authoritative.
- [ ] Claim due work with database row locking/conditional state checks and
      `SKIP LOCKED` batching; prove concurrent workers create one resume.
- [ ] Define owner validity separately from owner liveness: valid offline owners
      stay pinned and receive queued outbox resumes; invalid/revoked owners get
      terminal `owner_unavailable` policy with no migration.
- [ ] Create resumes for valid offline pinned owners through the durable outbox;
      never migrate. Prove reconnect delivery and due-worker races do not create
      extra attempts.

## 6. Clarification lifecycle and cancellation

- [ ] Keep Retrying runs active and sequential-slot occupying until retry policy
      terminalizes them.
- [ ] Define running, retry-waiting, due, owner-valid/offline,
      owner-invalid/revoked, unknown-outcome, and cancellation races; stale work
      must not resurrect a run or resume a terminal run.
- [ ] Preserve explicit run identity and make cancellation unable to affect a
      newer run.

## 7. Public projection and browser behavior

- [ ] Extend existing session read with safe `attempt_count`, `next_retry_at`,
      `failure_reason`, `retrying`, and `failed` mappings; expose no raw runtime
      or daemon/provider detail.
- [ ] Update workspace API types/rendering for active retry and terminal failure
      without adding an execution-state endpoint or browser auto-retry.
- [ ] Add Vitest projection/error tests and Playwright refresh/reconnect,
      retrying-slot, terminal-failure, and cancellation tests.

## 8. Integration, architecture, and docs

- [ ] PostgreSQL integration: restart recovery, duplicate events, concurrent
      due workers, command/attempt atomicity, pinned owner, cancellation races,
      and Requirement isolation.
- [ ] Architecture tests continue to reject daemon retry authority and browser
      WebSocket/migration paths.
- [ ] Update protocol, daemon, persistence, architecture, lifecycle, testing,
      and invariant docs with honest pending/enforced statuses.
- [ ] Run targeted tests, `openspec validate --all --strict`, and relevant
      `scripts/validate.sh` profiles; do not check unexecuted layers.
