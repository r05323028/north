## Purpose

Separates infrastructure health from business truth: the server persists execution state and retry policy, daemon recovery stays local, and Requirement lifecycle remains untouched.

## ADDED Requirements

### Requirement: Server owns execution state and retry policy

For every active session, the server SHALL authoritatively persist execution
state (`Idle`, `Running`, `Retrying`, or `Failed`), attempt count, retry budget,
and failure reason. The server SHALL decide whether to issue `session.resume`
and when an execution becomes `Failed`. The attempt count SHALL survive server
and daemon restarts and SHALL increment only for a server-directed execution
start/resume attempt, not for an Axum/`tokio-tungstenite` WebSocket reconnect,
frame replay, North heartbeat, or local runtime transport recovery.

#### Scenario: Daemon restart does not reset attempts

- **WHEN** a daemon restarts after two server-directed attempts
- **THEN** the server still reports attempt count 2 and applies the remaining budget

#### Scenario: Exhaustion is decided by server

- **WHEN** recoverable failure facts exhaust the persisted server retry budget
- **THEN** the server changes execution state to Failed and records the reason without changing Requirement lifecycle state

### Requirement: Daemon owns only transport and local recovery mechanics

The daemon MAY reconnect its `tokio-tungstenite` WebSocket with backoff, replay
buffered events, reattach a local runtime transport when instructed, and report
recoverability or
failure facts. It MUST NOT own a separate business retry budget, decide
permanent execution failure, or mutate Requirement lifecycle state. A daemon
`session.failed` frame is a fact report, not an authoritative server state
transition.

#### Scenario: Socket backoff is not a business attempt

- **WHEN** the daemon performs five WebSocket reconnects before one successful server-directed resume
- **THEN** the server attempt count increases only for that resume attempt

#### Scenario: Failure fact leaves business state alone

- **WHEN** the daemon reports a non-recoverable runtime failure
- **THEN** the server applies its own execution policy and the Requirement status, revision, and assessment remain unchanged

### Requirement: Retry policy is server configuration and durable state

Retry bounds and delays SHALL be server configuration with documented defaults,
and the current budget/attempt state SHALL survive process restarts. The daemon
MUST NOT receive or independently exhaust a second business-level budget.

#### Scenario: Operator tunes server patience

- **WHEN** the server's configured max attempts is lowered before a flaky run
- **THEN** server behavior follows the new bound while daemon transport backoff remains independent
