## Purpose

Makes forgetting safe: allowlisted ephemeral runtime telemetry expires on a
schedule while durable product and coordination state remain structurally
untouchable.

## ADDED Requirements

### Requirement: Retention applies only to explicitly allowlisted ephemeral data

Only data classes on the explicit ephemeral allowlist SHALL be eligible for
expiry-driven deletion. The 0.1.0 allowlist contains exactly
`clarification_activities`: coarse runtime activity telemetry that is never
the sole source of truth for any Requirement or execution decision. Every
other persisted class SHALL be durable by default, including business state
(users, roles, requirements, conversations, messages, readiness evidence,
review decisions, repositories) and coordination state (daemon registrations
and setup rows, execution sessions, execution attempts, attempt counters and
limits, `next_retry_at`, current-attempt identity, failure classification,
server command outbox and tombstones, command/event dedupe and rejection
records, and sequence watermarks). A table SHALL NOT become a retention
target through a generic or inferred mechanism; adding one SHALL require
deliberately extending the allowlist and its firewall test.

#### Scenario: Sweep deletes only its class

- **WHEN** the retention sweep runs with eligible expired activity rows and durable rows present
- **THEN** only expired allowlisted activity rows are deleted and every other table's rows and values are unchanged

#### Scenario: Retry and failure state survives a full purge

- **WHEN** every eligible ephemeral row is purged while retrying runs, execution attempts, counters, and due scheduling exist
- **THEN** attempt history, counters, limits, `next_retry_at`, and failure classification are identical afterward

### Requirement: Ephemeral expiry is explicit and deterministic

Allowlisted ephemeral rows SHALL carry `expires_at`, written at insert time
from the configured retention window; eligibility SHALL be
`expires_at <= CURRENT_TIMESTAMP`. Existing rows SHALL receive deterministic
retention at migration time (`expires_at = created_at` plus the default
window), and the column SHALL be NOT NULL so no future insert can bypass
expiry treatment.

#### Scenario: Expired activity is eligible, future activity is not

- **WHEN** the sweep runs with one activity row already expired and one row whose expiry is in the future
- **THEN** the expired row is deleted and the future row remains

#### Scenario: Migration backfills existing activity deterministically

- **WHEN** migration 0017 upgrades a database that already has activity rows
- **THEN** every existing row receives its created time plus the default window, and later inserts without an expiry are rejected

### Requirement: Sweeps are bounded, ordered, and idempotent

Each sweep pass SHALL delete at most the configured batch bound, order
candidates deterministically by expiry then identity, and use PostgreSQL row
locks (`FOR UPDATE SKIP LOCKED`) so concurrent server instances delete
disjoint rows without duplication. A pass SHALL stop at its bound even when
more expired rows remain. Sweeps SHALL be idempotent, SHALL never delete
non-expired rows, and SHALL not require replay or repair after a restart.

#### Scenario: Batch bound is honored

- **WHEN** more expired rows exist than the configured batch bound
- **THEN** one pass deletes exactly the bound and later passes continue with the remainder

#### Scenario: Concurrent sweeps are safe

- **WHEN** two server instances sweep the same expired set at the same time
- **THEN** each row is deleted at most once, neither sweep errors, and a repeated sweep drains only what remains

### Requirement: Backlog recovery is bounded per cycle

One scheduler cycle SHALL drain expired telemetry by running bounded sweep
passes while a pass deleted a full batch, SHALL stop when a pass deletes fewer
rows than the batch bound, and SHALL stop at the configured maximum number of
passes per cycle. Each pass SHALL remain an independent batch-bounded
statement so a cycle never becomes one large transaction, and retention SHALL
never run an unbounded loop. When the pass bound stops a cycle with expired
rows still eligible, the cycle SHALL report that expired rows remain, using a
bounded existence probe rather than an exact backlog count, and the next cycle
continues the drain. Exact backlog size is an observability concern and SHALL
not be coupled to retention progress.

#### Scenario: Backlog larger than one batch recovers

- **WHEN** a cycle starts with more expired rows than one batch bound
- **THEN** the cycle runs several bounded passes, deletes the whole backlog, and stops when the backlog is exhausted

#### Scenario: Exact cycle capacity does not report a limit

- **WHEN** a cycle consumes exactly its configured capacity and no expired rows remain
- **THEN** the cycle reports that the drain limit was not reached

#### Scenario: Pass bound stops the cycle and reports that rows remain

- **WHEN** a cycle reaches the configured maximum pass count with expired rows still eligible
- **THEN** the cycle stops, reports only that expired rows remain after a bounded existence probe, and a later cycle drains the remainder

#### Scenario: Restart preserves drain progress

- **WHEN** the server restarts between cycles and a new cycle runs
- **THEN** it continues from the persisted expiry state, deleting only eligible rows and reporting zero once the backlog is empty

### Requirement: Deletion cannot change canonical product or coordination state

Purging every eligible ephemeral row SHALL leave every canonical projection
semantically identical: Requirement detail and board/list reads, durable
conversation history, readiness evidence and review-packet projection,
clarification run phase and status projection, execution attempt history,
retry and failure state, repository provenance, and durable protocol state
needed for reconnect and reconciliation. Only activity telemetry may
disappear.

#### Scenario: Amnesia test

- **WHEN** representative state containing requirement, conversation, readiness, review, run, attempt, retry, repository, and protocol rows is purged of all eligible activity telemetry
- **THEN** every canonical projection is identical before and after, and only activity rows are gone

### Requirement: Retention configuration is typed, bounded, and validated

Retention SHALL be configured by a typed settings value with bounded settings:
retention window, sweep cadence, per-pass batch bound, and maximum passes per
cycle. The 0.1.0 defaults SHALL be 7 days, 60 seconds, 500 rows, and 20
passes. Invalid values — a non-positive window or cadence, a batch bound or
pass bound outside the allowed range — SHALL be rejected explicitly rather
than clamped or ignored, and the server SHALL fail closed instead of running
with invalid retention settings.

#### Scenario: Invalid configuration is rejected

- **WHEN** retention settings with a non-positive window, non-positive cadence, or out-of-range batch or pass bound are constructed
- **THEN** construction fails with an explicit error and no sweep can run from those settings

#### Scenario: Defaults are valid

- **WHEN** server startup builds retention settings without overrides
- **THEN** the documented defaults are accepted and used

### Requirement: The sweep layer cannot target durable tables by construction

The persistence API SHALL expose only named ephemeral maintenance operations
with fixed SQL; it SHALL NOT expose generic table-name deletion or arbitrary
SQL deletion primitives to hosts. The TTL allowlist SHALL contain exactly
`clarification_activities`. Daemon setup-row retention and acknowledged outbox
compaction are separately classified bounded deletions, not TTL targets.
A structural test SHALL inspect persistence Rust syntax and require every
destructive statement (DELETE, TRUNCATE) to be a static string literal passed
directly to a known SQL-executing call, classifying it by exact source file and
table; SQL assembled through `format!`, `concat!`, variables, or dynamic table
names SHALL be rejected, as SHALL any unclassified durable table, and test
modules SHALL not satisfy production classification.

#### Scenario: Durable-table delete fails the firewall

- **WHEN** persistence code adds a deletion for a table outside the classified allowlist
- **THEN** the architecture firewall test fails and names the offending file and table

#### Scenario: Dynamic construction cannot hide a deletion

- **WHEN** a destructive statement is built with `format!` or `concat!`, held in a variable, split across lines, written in lowercase, or uses irregular whitespace
- **THEN** the firewall classifier rejects it instead of treating it as a classified static literal

#### Scenario: TRUNCATE is always rejected

- **WHEN** persistence code executes TRUNCATE through any known SQL call
- **THEN** the firewall test fails and names the offending file
