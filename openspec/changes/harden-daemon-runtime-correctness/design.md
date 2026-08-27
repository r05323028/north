## Context

See `proposal.md` for motivation and scope. The current runtime stores setup,
auth, and command state in PostgreSQL; browser routes use Axum middleware and
sessions, while `north-daemon` uses a small `curl`-based device-flow client.
The server is single-process in North 0.1.0, and persistence cannot depend on
wire/protocol crates.

## Goals / Non-Goals

**Goals:**

- Make only POST approval state-changing and reject cross-origin browser
  approval without changing `SameSite=Lax` cookies.
- Make verification-attempt accounting atomic and bounded.
- Ensure one complete command envelope is serialized before its exact payload is
  persisted, then dispatch the persisted envelope.
- Bound setup-row retention, invalidate old connection leases at startup, and
  retry transient setup polling failures.
- Preserve current connection IDs, heartbeat liveness, session pinning,
  credential revocation, protocol phases, and architecture boundaries.

**Non-Goals:**

- Redis, a background scheduler, HA ownership epochs, durable redelivery, or a
  new HTTP client dependency.
- Changes to browser cookie policy or unrelated role/auth behavior.

## Decisions

### Approval confirmation and origin check

Split the approval route into a read-only GET preview and a POST approval
handler. Persistence exposes a non-mutating setup-request preview containing
only label and lifecycle state; the HTTP response returns JSON confirmation
state. The POST requires `CurrentUser` from existing auth middleware and a
present `Origin` whose scheme is HTTP(S) and whose host (including port) equals
the request `Host`. This host-bound check rejects cross-site browser requests
without trusting arbitrary forwarded headers or weakening cookies. The CLI
never approves, so it does not need a CSRF token.

Alternative: a synchronizer token would require rendering and retaining a
browser form token. Origin validation is sufficient for this JSON endpoint and
keeps the 0.1.0 stack small.

### Transactional verification budget

Add `failed_attempts INTEGER NOT NULL DEFAULT 0` to `verification_codes` and
set the limit to five. `verify_code` locks the newest active, unexpired row
with `FOR UPDATE`. A mismatch increments the row in that transaction and marks
it consumed when the new count reaches five; the transaction commits before
returning `InvalidCode`. A matching code consumes the row and creates the
session in the existing transaction. A new issued code still supersedes the
active row and starts at zero. The existing per-email advisory lock and
request cooldown remain separate.

Alternative: an in-memory counter would not survive process restart and would
allow concurrent database-backed requests to bypass the limit.

### Exact command persistence and dispatch

Keep protocol types in `north-protocol` and persistence independent of them.
Change the persistence session-start operation to accept a small synchronous
payload factory invoked after it has selected/pinned the daemon and assigned
the next sequence while holding the transaction. The factory receives daemon
ID and sequence and returns the complete serialized payload; that exact string
is inserted and returned in `PinnedCommand`.

Expose a server-level typed `CommandRequest` containing command ID, session ID,
and `north_protocol::Command`. `DaemonRuntime::persist_and_dispatch_command`
passes a factory that builds `CommandEnvelope` and `ServerFrame::Command`, then
parses the returned stored payload and sends that parsed frame through the
existing live connection. The raw `ServerFrame` dispatch helper becomes
private. Thus callers cannot pair an arbitrary persisted payload with an
unrelated public raw frame, and a failed dispatch leaves the stored payload
unchanged.

Alternative: make persistence depend on `north-protocol`; rejected because it
would violate the persistence/protocol boundary and pull runtime concerns into
storage.

### Setup cleanup

Add an expiry index and a migration column in one forward migration. A bounded
CTE deletes at most 100 rows whose expiry is older than 24 hours. Creation and
polling invoke this lightweight cleanup opportunistically; the indexed
predicate avoids full-table deletion. No scheduler is added.

### Restart lease invalidation

Add `AuthStore::invalidate_daemon_connections`, clearing `connected_at` and
`connection_id` for all registrations. `build_app` runs it after migrations
and before mounting/serving the router. Startup propagates migration and
invalidation errors through a small server startup error type. Direct test
routers can call the same operation explicitly; no asynchronous cleanup is
spawned after serving begins.

### Polling retry

Retain `curl` to avoid a new dependency. Have `curl_json` capture the HTTP
status as a trailing write-out field and classify connection/timeout failures
and 5xx responses as retryable; classify 4xx and malformed successful bodies
as terminal. The setup poll loop retries retryable errors with a bounded
exponential delay capped below the setup deadline, resets delay after a good
poll, and reports terminal or expiry failures clearly.

## Risks / Trade-offs

- **Strict Origin requirement can reject non-browser approval clients** → the
  approval operation is explicitly browser-authenticated; tests and supported
  clients send Origin, while setup request and claim remain CLI/public routes.
- **Bounded opportunistic cleanup can leave recent expired rows** → retain rows
  for 24 hours for diagnostics and remove at most 100 per invocation; future
  deployments can add scheduled maintenance if volume requires it.
- **Command factory is a persistence callback rather than a shared protocol
  type** → it keeps crate boundaries intact; the server owns serialization and
  the returned stored payload is the sole dispatch source.
- **Startup invalidation briefly marks all daemons offline** → this is the
  intentional single-server 0.1.0 restart contract; daemon reconnect is
  required. Multi-server ownership epochs remain future work.

## Migration Plan

1. Apply the forward migration through existing startup migration handling.
2. Invalidate old daemon connection leases before accepting new HTTP traffic.
3. Deploy server and daemon binaries; existing setup, auth, and reconnect flows
   continue with the new checks.
4. Roll back application code only after preserving the forward schema; the
   added column and index are backward-compatible with reads by prior binaries.
