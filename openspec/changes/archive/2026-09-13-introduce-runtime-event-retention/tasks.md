## 1. Mechanics

- [x] 1.1 Migration 0017: nullable `expires_at` on `clarification_activities`, deterministic backfill (created time plus 7-day default), NOT NULL enforcement, sweep index
- [x] 1.2 Activity insert path writes `expires_at` from the configured retention window
- [x] 1.3 Named bounded sweep operation in persistence (ordered, `FOR UPDATE SKIP LOCKED`, no generic delete API)
- [x] 1.4 `RetentionConfig` with validating constructor and documented 0.1.0 defaults
- [x] 1.5 Retention ticker wired beside the retry worker; failures logged and retried next tick
- [x] 1.6 `build_app_with_retention` exposes validated settings; `build_app` delegates with defaults; prospective semantics documented
- [x] 1.7 Bounded multi-pass drain cycle: runs multiple batch-sized passes per cycle, stops on a partial batch, respects the configured maximum passes, uses a bounded post-pass backlog existence probe instead of an exact unbounded count, and reports no remaining backlog when an exact-capacity cycle drains all eligible rows

## 2. Proofs

- [x] 2.1 Amnesia integration test: purge all eligible telemetry, then Requirement/board/conversation/readiness/review/session/attempt/retry/repository/user/setup/outbox/dedupe/watermark projections identical
- [x] 2.2 Expiry boundary, batch bound, repeated sweep, concurrent sweep, empty sweep, and post-restart sweep tests
- [x] 2.3 Invalid and default retention configuration tests
- [x] 2.4 Durable-table firewall structural test: syn-based, static-literal classification per (file, table), dynamic/TRUNCATE rejection, with adversarial tests for lowercase, multiline, whitespace, `format!`, `concat!`, variables, dynamic table names, and unclassified tables
- [x] 2.5 Migration upgrade test: legacy activity backfill deterministic, NOT NULL and sweep index enforced
- [x] 2.6 Custom-window store asserts `expires_at - created_at` follows configured retention
- [x] 2.7 Human review decision recorded on a second Ready Requirement while the first keeps its review packet
- [x] 2.8 Replayed purged activity event returns duplicate outcome without recreating telemetry or changing watermarks/dedupe
- [x] 2.9 Drain tests: backlog larger than one batch recovers, exhaustion stops the cycle, configured pass bound is respected with observable remainder, non-expired rows survive, concurrent drains delete each row once, post-restart drain is idempotent

## 3. Docs

- [x] 3.1 docs/architecture/persistence.md: allowlist, 0017, sweep and drain mechanics, durable retry/attempt exclusion
- [x] 3.2 docs/development/testing.md, ci.md, and invariants.md rows updated with real enforcement
- [x] 3.3 Canonical capability deltas for `clarification-runtime` and `requirement-conversation-workspace`

## 4. Validation

- [x] 4.1 `./scripts/validate.sh fast` (executed 2026-09-13, passed: rust fmt/clippy/unit+arch, web lint/typecheck, openspec strict)
- [x] 4.2 PostgreSQL integration suites (`retention`, `retry_authority`, `migration_upgrade`) with `NORTH_TEST_DATABASE_URL` (executed 2026-09-13 against isolated fresh databases: 9 + 5 + 1 tests passed)
- [x] 4.3 `openspec validate --all --strict`, `git diff --check`, pre-push validation (executed 2026-09-13: strict validation 29/29, clean diff check, `./scripts/pre-push-validation.sh` native gate OK plus act rust job parity succeeded with `-P self-hosted=catthehacker/ubuntu:act-latest`)
