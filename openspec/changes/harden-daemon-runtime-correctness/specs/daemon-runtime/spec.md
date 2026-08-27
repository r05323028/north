## Purpose

Keeps North's authenticated daemon runtime safe and coherent at HTTP,
persistence, restart, lifecycle, and client-retry boundaries without changing
its single-server 0.1.0 architecture.

## ADDED Requirements

### Requirement: Setup approval is safe against cross-site state changes

`GET /daemon/setup/{request_token}/approve` SHALL be read-only and SHALL NOT
approve, claim, or otherwise mutate a setup request. It SHALL return a
confirmation state that identifies the daemon label when available. The
state-changing approval SHALL be performed only by
`POST /daemon/setup/{request_token}/approve`, which SHALL require an
authenticated user and SHALL validate the browser request origin against the
North server origin before mutating state. Invalid, expired, already claimed,
unauthenticated, and cross-origin approval requests SHALL be rejected without
creating a credential.

#### Scenario: Approval page is read-only

- **WHEN** an authenticated user requests the approval URL with `GET`
- **THEN** the response shows confirmation state and daemon information, but
  the setup request remains pending and no credential is created

#### Scenario: Valid same-origin POST approves

- **WHEN** an authenticated user submits a valid setup token with a valid
  same-origin approval `POST`
- **THEN** the request is approved and the one-time daemon credential can be
  claimed exactly once

#### Scenario: Unauthenticated POST is rejected

- **WHEN** a client submits an approval `POST` without an authenticated
  session
- **THEN** the server rejects it and leaves the setup request pending

#### Scenario: Cross-origin approval is rejected

- **WHEN** a browser submits an approval `POST` with an origin different from
  the North server origin
- **THEN** the server rejects it and leaves the setup request pending

#### Scenario: Cross-site GET cannot approve

- **WHEN** a top-level cross-site navigation sends `GET` to an approval URL
  with a valid session cookie
- **THEN** the server returns read-only confirmation state and does not approve
  the setup request

### Requirement: Durable command envelope matches dispatch

For any command persisted to the daemon outbox, the server SHALL construct one
complete command envelope, including command ID, session identity, and
sequence metadata, serialize that envelope, persist that exact representation
transactionally, and dispatch that same persisted representation. Callers
SHALL NOT be able to persist an arbitrary payload and dispatch an unrelated
command through the public command path. Persistence SHALL complete before
attempted dispatch; a dispatch failure SHALL NOT rewrite the persisted command
or silently substitute another representation.

#### Scenario: Stored command is the received command

- **WHEN** the server starts a pinned session with a typed command
- **THEN** the outbox payload decodes to the same command envelope received by
  the daemon, including command ID and sequence metadata

#### Scenario: Persistence precedes dispatch

- **WHEN** command persistence succeeds and dispatch is attempted
- **THEN** the daemon receives the persisted envelope, and an observed dispatch
  failure leaves that exact envelope in the outbox

#### Scenario: Session ownership remains pinned

- **WHEN** a command is created for an active session
- **THEN** daemon selection and session pinning follow existing eligibility and
  connection-identity rules without allowing a foreign daemon to receive it

### Requirement: Expired setup requests have bounded retention

The server SHALL opportunistically remove setup-request rows whose expiry is
older than a bounded retention window when setup requests are created or
polled. Cleanup SHALL be bounded per invocation and use the expiry index; it
MUST NOT scan and delete the entire setup-request table on every request.
Recent expired rows MAY remain during the retention window for diagnostics.

#### Scenario: Expired rows are eventually removed

- **WHEN** setup request creation or polling occurs after a row is expired
  beyond the retention window
- **THEN** that row is removed while recent expired rows may remain

#### Scenario: Cleanup is bounded

- **WHEN** many setup requests exist and cleanup runs
- **THEN** one invocation removes at most its configured batch and does not
  require an unbounded full-table scan

### Requirement: Restart invalidates stale daemon leases

In single-server North 0.1.0, server startup SHALL invalidate persisted daemon
connection state from a prior server process before accepting new session
placement. A daemon SHALL reconnect and establish a new live connection before
becoming eligible. Existing connection-ID race protection and clean disconnect
cleanup SHALL remain effective. Multi-server ownership epochs and HA lease
coordination are deferred.

#### Scenario: Stale state is not eligible after restart

- **WHEN** a daemon is marked connected, server runtime state is restarted,
  and that daemon has not reconnected
- **THEN** new session placement rejects the daemon as ineligible

#### Scenario: Reconnect restores eligibility

- **WHEN** the daemon reconnects after restart with valid credentials
- **THEN** its new connection state becomes eligible under the existing
  heartbeat liveness window

#### Scenario: Clean disconnect still clears state

- **WHEN** an active daemon connection closes normally
- **THEN** its persisted connection state is cleared without weakening
  connection-ID race protection

### Requirement: Daemon setup polling retries transient failures

The `north-daemon setup` polling loop SHALL retry transient connection failures
and retryable HTTP 5xx responses using bounded polling intervals and backoff.
It SHALL stop on successful completion, setup expiry, terminal client/protocol
errors, or exhausted bounded retry/deadline policy, and SHALL report a useful
final error. Polling retries SHALL NOT create additional setup requests.

#### Scenario: Connection interruption is retried

- **WHEN** a setup status poll fails because of a transient connection error
- **THEN** the daemon waits within its bounded backoff and retries while the
  setup request remains valid

#### Scenario: Server failure is retried

- **WHEN** a setup status poll receives an HTTP 5xx response
- **THEN** the daemon retries within the bounded polling policy

#### Scenario: Terminal failure stops polling

- **WHEN** a setup status poll receives a terminal 4xx or invalid protocol
  response
- **THEN** polling stops and reports the failure without retrying forever

#### Scenario: Expiry stops polling

- **WHEN** the setup request reaches its expiry before approval completes
- **THEN** polling stops with an expiry error
