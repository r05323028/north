## 1. Canonical capability cleanup

- [x] Replace the overlapping `execution-state` delta with a modification of
      `openspec/specs/execution-retry-authority`.
- [x] Align clarification-runtime and Requirement workspace contracts with
      active `retrying` and terminal `failed` projection semantics.
- [x] Update the daemon-protocol delta so the landed clarification event
      projection keeps identity, sequence, dedupe, and ACK-after-commit while
      only `session.failed` changes to execution-attempt retry/terminal policy;
      do not duplicate generic projection routing.
- [x] Keep session ownership/daemon-runtime as consumed boundaries; remove
      stale wording that treats retry behavior as a future duplicate capability.

## 2. Durable persistence and transaction boundaries

- [x] Add migration fields for attempt count, snapshotted max attempts,
      `next_retry_at`, safe failure class/reason, and current attempt identity.
- [x] Add narrow `execution_attempts` persistence with unique session/attempt,
      command, and failure-event identities; add due-retry index.
- [x] Make initial `session.start` and every `session.resume` commit command,
      attempt row, counter update, and current-attempt identity atomically.
- [x] Make accepted `session.failed` close/clear current attempt N before
      entering Retrying or terminal Failed; due scheduling requires no current
      attempt and makes N+1 current.
- [x] Backfill existing sessions conservatively and document restart behavior.

## 3. Attempt identity/accounting

- [x] Count initial start and each new resume exactly once at durable command
      creation.
- [x] Prove reconnect, command ACK retry, daemon journal replay, event replay,
      heartbeat, reconciliation, cancel, and message commands do not count.
- [x] Keep attempt identity run-scoped and prevent a delayed old run from
      affecting a newer run.

## 4. Failure facts and policy

- [x] Process `session.failed` as an attempt fact after event identity/sequence
      validation and classify to bounded safe reasons.
- [x] Implement known-failure retry/exhaustion transitions without Requirement
      mutation. Once unknown outcome terminalizes a run, prohibit all later
      `session.resume`; cover new-run/new-start recovery with current context and
      normal slot/state-version rules.
- [x] Make duplicate/replayed failure facts return the original ACK/outcome and
      perform no second budget, schedule, resume, or terminal effect.

## 5. Durable scheduler and pinned ownership

- [x] Implement startup plus bounded polling/wakeup discovery of due rows; do
      not make in-memory timers authoritative.
- [x] Claim due work with database row locking/conditional state checks and
      `SKIP LOCKED` batching; prove concurrent workers create one resume.
- [x] Define owner validity separately from owner liveness: valid offline owners
      stay pinned and receive queued outbox resumes; invalid/revoked owners get
      terminal `owner_unavailable` policy with no migration.
- [x] Create resumes for valid offline pinned owners through the durable outbox;
      never migrate. Prove reconnect delivery and due-worker races do not create
      extra attempts.

## 6. Clarification lifecycle and cancellation

- [x] Keep Retrying runs active and sequential-slot occupying until retry policy
      terminalizes them.
- [x] Define running, retry-waiting, due, owner-valid/offline,
      owner-invalid/revoked, unknown-outcome, and cancellation races; stale work
      must not resurrect a run or resume a terminal run.
- [x] Preserve explicit run identity and make cancellation unable to affect a
      newer run.

## 7. Public projection and browser behavior

- [x] Extend existing session read with safe `attempt_count`, `next_retry_at`,
      `failure_reason`, `retrying`, and `failed` mappings; expose no raw runtime
      or daemon/provider detail.
- [x] Update workspace API types/rendering for active retry and terminal failure
      without adding an execution-state endpoint or browser auto-retry.
- [x] Add Vitest projection/error tests and Playwright refresh/reconnect,
      retrying-slot, terminal-failure, and cancellation tests.

## 8. Integration, architecture, and docs

- [x] PostgreSQL integration: restart recovery, duplicate events, concurrent
      due workers, command/attempt atomicity, pinned owner, cancellation races,
      and Requirement isolation.
- [x] Architecture tests continue to reject daemon retry authority and browser
      WebSocket/migration paths.
- [x] Update protocol, daemon, persistence, architecture, lifecycle, testing,
      and invariant docs with honest pending/enforced statuses.
- [x] Run targeted tests, `openspec validate --all --strict`, and relevant
      `scripts/validate.sh` profiles; do not check unexecuted layers.
