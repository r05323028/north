# execution-retry-authority Specification

## Purpose

Separates daemon transport recovery from server execution policy so attempt budgets, retry decisions, and terminal failure survive reconnects and daemon restarts.

## Requirements

### Requirement: Server owns execution state and retry policy

For every active session, the server SHALL authoritatively persist execution
state (`Idle`, `Running`, `Retrying`, or `Failed`), attempt count, retry budget,
and failure reason. The server SHALL decide whether to issue
`session.resume` and when an execution becomes `Failed`. The attempt count
SHALL survive server and daemon restarts and SHALL increment only for a
server-directed execution start/resume attempt, not for a WebSocket reconnect,
frame replay, heartbeat, or local runtime transport recovery.

#### Scenario: Daemon restart does not reset attempts

- **WHEN** a daemon restarts after two server-directed attempts
- **THEN** the server still reports attempt count 2 and applies the remaining budget

#### Scenario: Exhaustion is decided by server

- **WHEN** recoverable failure facts exhaust the persisted server retry budget
- **THEN** the server changes execution state to Failed and records the reason without changing Requirement lifecycle state

### Requirement: Daemon owns only transport and local recovery mechanics

The daemon MAY reconnect its WebSocket with backoff, replay buffered events,
reattach a local runtime transport when instructed, and report recoverability or
failure facts. `session.failed.recoverable` means only whether the existing
runtime operation can be safely resumed or reattached by daemon-local mechanics:
`true` means local recovery is believed possible; `false` means it is not. It
does not mean the server is forbidden from issuing a future explicit attempt and
does not authorize the daemon to decide authoritative execution failure. The
daemon MUST NOT own a separate business retry budget, decide permanent execution
failure, or mutate Requirement lifecycle state. A daemon `session.failed` frame
is a fact report, not an authoritative server state transition. For
`execution_outcome_unknown`, automatic daemon resubmission is forbidden; any
later attempt is explicitly server-directed, uses a new command identity, and
follows server retry policy.

#### Scenario: Socket backoff is not a business attempt

- **WHEN** the daemon performs five WebSocket reconnects before one successful resume
- **THEN** the server attempt count increases only for the server-directed resume attempt

#### Scenario: Recoverability does not decide retry

- **WHEN** the daemon reports `session.failed` with either
  `recoverable: true` or `recoverable: false`
- **THEN** the server treats that value as a local recovery fact and alone
  decides whether to issue a new `session.resume`/`session.start` command or
  mark execution Failed; an unknown outcome is never automatically resubmitted

#### Scenario: Failure fact leaves business state alone

- **WHEN** the daemon reports a non-recoverable runtime failure
- **THEN** the server applies its own execution policy and the Requirement status and revision remain unchanged
