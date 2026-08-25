## Context

See `proposal.md` for motivation. Current production crates are still mostly
scaffolds. Existing OpenSpec contracts describe daemon-event replay and a
reusable repository clone, but do not yet define the reverse command path,
per-session ordering, API compare-and-swap semantics, daemon pinning, or which
side owns execution retry decisions.

The design must preserve the existing crate graph and transport topology. The
server may use its relational database; the daemon may use a local durable
transport journal, but the daemon does not gain `north-persistence`, direct
server database access, or a broker.

## Goals / Non-Goals

**Goals:**

- Make every reconnect boundary and acknowledgement meaning explicit.
- Keep server business authority and daemon execution-host boundaries intact.
- Give later implementation changes one canonical contract and test plan.
- Enforce structural boundaries now where source and manifests already exist.

**Non-Goals:**

- Implementing the server, daemon, SQL migrations, or UI in this
  architecture-only change. The later transport slice may add only the thin
  Axum WebSocket adapter and `tokio-tungstenite` supervisor required to make
  the already-specified boundary executable.
- Kafka/NATS/Redis, an external job runner, object storage, a scheduler
  framework, live daemon migration, multi-user daemon sharing, Git worktrees,
  or kernel-level sandboxing.
- Turning SSE into browser event sourcing or exposing runtime internals.

## Decisions

### Canonical ownership

This change owns cross-cutting contracts in its seven capability specs.
Pending product changes reference them instead of restating incompatible
variants. Existing product changes still own their bounded implementation
surfaces: protocol types in `introduce-server-daemon-protocol`, session
connection/auth in `introduce-daemon-runtime-connection`, execution state in
`introduce-runtime-retry-and-failure-state`, repository catalog in
`introduce-configured-repositories`, inspection workspaces in
`introduce-local-repository-inspection`, and Requirement/readiness APIs in the
corresponding domain changes.

### Durable command delivery and acknowledgement

The server creates a command outbox row in the same transaction that creates
the business intent. The row contains `command_id`, pinned `daemon_id`,
`session_id`, `server_command_seq`, command type/payload, and delivery status.
The unique session/sequence and command-id constraints prevent two outbox rows
from representing one command. A dispatcher sends pending and unaccepted rows
again after reconnect; it never allocates a new id for a retry.

The daemon keeps a local append-only transport journal for command inbox and
event replay records. A journal append is flushed before the daemon sends
`command.accepted`. This is transport durability, not a business database.
The command ledger records `accepted`, `dispatch_started`, and terminal outcome
states. A unique command id makes duplicate frames return the existing
acceptance/outcome. The daemon writes `dispatch_started` before invoking the
runtime, and passes `command_id` as the runtime operation id. A restart
reattaches to that operation when possible; it never automatically invokes a
`dispatch_started` command a second time. If the operation cannot be
reconciled, the daemon reports an explicit unknown outcome and the server's
policy handles it; side-effecting `message.send` is not automatically resent
under a new id.

`command.accepted` is the only command delivery ACK in 0.1.0. It means durable
inbox acceptance, not runtime completion. Runtime completion/failure remains
represented by session/event facts and server execution state. Thus the server
may remove the command payload from its active outbox after accepted ACK while
retaining session sequence watermarks and outcome history.

The daemon may compact individual inbox rows only after terminal session
reconciliation proves a contiguous processed sequence. It retains a durable
`processed_through_seq` high-water mark for the session and any sparse rows
above it. No time-based expiry is allowed for that tombstone in 0.1.0.

Daemon event handling mirrors this boundary in the other direction. The daemon
journals an event and assigns `daemon_event_seq` before sending it. The server
processes an event in the same persistence transaction as its dedupe marker and
business effect. Valid effects receive an `event.accepted` ACK after commit.
Well-formed but permanently rejected facts (for example a stale assessment)
commit a durable rejection/dedupe record and receive `event.rejected`; this is
an acknowledgement of handled rejection, not successful business mutation.
Crashes or transient failures before either commit produce no ACK, so replay is
safe.

### Sequence and reconciliation rules

`server_command_seq` and `daemon_event_seq` are independent counters scoped to
one session and direction. The assigning side persists the next value with the
outbox/journal record. Reconciliation exchanges contiguous watermarks and
sparse sequence sets where processing is non-contiguous. A receiver buffers a
valid out-of-order frame but does not apply it until missing sequence values
arrive. Same id plus same sequence is a duplicate; same sequence plus another
id is a protocol error; an already acknowledged late frame is inert and
re-acknowledged. This keeps ids for idempotency and sequences for order/gap
detection.

### Protocol compatibility

The daemon hello and server welcome carry exact `protocol_version: "0.1"`.
Frames carry `schema_version: 1`. There is no generalized range negotiation in
0.1.x. Version mismatch, unknown frame type, or unsupported schema gets an
explicit `protocol.error` and connection close before side effects; unaccepted
outbox/journal records remain replayable. This fails closed rather than
silently guessing a payload shape.

### Requirement concurrency and assessment commit

The API contract requires `expected_revision` on every mutation of an
existing Requirement. Persistence uses one atomic compare-and-swap update or
row-lock transaction; a zero-row update maps to a typed conflict and HTTP 409.
The domain aggregate remains the source of lifecycle rules and is called only
after the current row is atomically claimed. No new domain setter or
infrastructure dependency is needed.

`requirement.assessed` uses one server transaction: dedupe event, lock/current
claim, compare event revision, run domain gates, write immutable evidence and
its accepted/rejected validation result, apply a valid transition, persist the
Requirement, commit, then emit the event ACK. A duplicate committed event
repeats only its ACK. A stale or invalid event commits a rejection/dedupe
record with no Requirement transition, then emits `event.rejected`; a crash
before that commit emits no ACK.

### Session ownership and retry authority

A session may be created as an unstarted record, but the server atomically
selects an eligible live daemon and stores `session.daemon_id` before the first
command. Selection is a simple capability/repository filter, not a scheduler.
All routing and event authentication checks the pinned identity. Reconnect may
resume only against that identity. Revocation or outage never causes live
migration.

Daemon registrations are instance-scoped identities with user-owned
credentials. `created_by` is the account that authorized setup and owns the
credential. The owner may revoke its own credential; Admin/Owner may revoke
any. Revocation closes live access and leaves pinned sessions for normal server
retry/failure handling.

The server persists execution state, attempt count, budget, and reason. One
attempt means one server-directed execution start/resume, not one socket
connection or frame replay. Server alone sends `session.resume` and declares
`Failed`. Daemon reconnect/backoff, event replay, and local runtime reattach
are transport/local recovery facts and never consume the server budget.

### Repository identity and workspace isolation

The configured repository row gains nullable `disabled_at`; normal Remove
sets it, and normal catalog/inspection queries exclude disabled rows. The row
remains available for historical assessment joins and contains no credentials.
Assessment evidence keeps repository id and full commit SHA.

The daemon's reusable per-repository cache is a source for creating a unique
session/task checkout, never the runtime's mutable directory. A plain local
copy/clone-from-cache is sufficient; Git worktrees are not needed. Every
checkout gets dirty-tree validation and is deleted after the task. A dirty
result is an invariant violation and is reported after disposal. This is
process-level defense only; it does not claim kernel isolation.

### SSE reconnect

SSE carries notification hints, optionally with lightweight ids. EventSource
reconnect and `Last-Event-ID` are allowed optimizations, not correctness
requirements. After connect/reconnect/page load, the browser fetches canonical
Requirement/conversation/execution state over HTTP. Missed or duplicate hints
only cause a harmless refetch; no browser code opens WebSockets.

### Architecture enforcement

The existing effective Cargo-metadata dependency test remains the source of
truth for crate edges. Add a narrow source scan for daemon declarations of
server-owned execution/retry authority, while explicitly allowing transport
backoff names. Keep credential-schema checks ready for the repository schema
once it exists; do not pretend a check against nonexistent server code proves
anything. Behavioral guarantees receive integration/E2E tests in their owning
pending changes, not architecture-test substitutes.

## Risks / Trade-offs

- **Crash after `dispatch_started` but before runtime outcome** → persist the
  operation id first, reattach when possible, otherwise report unknown and do
  not auto-resubmit side-effecting commands.
- **Durable ledgers can grow** → compact only to per-session sequence
  watermarks; retain the small tombstone for the durable session.
- **No SSE replay can leave a stale browser briefly** → refetch canonical state
  after every reconnect and treat stream data as a nudge only.
- **Disposable checkout is not a sandbox** → retain dirty-tree detection and
  state the process-level limit in docs; defer stronger isolation until a real
  threat model requires it.
- **Cross-cutting contracts span pending changes** → keep one canonical spec
  here and add explicit references/tasks to each affected change.

## Migration Plan

This change first updates contracts, docs, and structural tests. Later owning
changes add the server outbox/session/revision migrations, daemon journal,
protocol frames, repository `disabled_at`, and browser refetch behavior in
small slices with integration tests. A deployment that cannot negotiate
protocol `0.1` is rejected rather than partially upgraded. Documentation-only
changes roll back by reverting the contract/docs commit; no production schema
is changed here.
