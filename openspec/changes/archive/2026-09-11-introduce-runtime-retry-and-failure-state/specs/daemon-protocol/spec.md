# daemon-protocol Specification Delta

## MODIFIED Requirements

### Requirement: Runtime events use owning clarification projection

The canonical daemon-protocol requirement already routes clarification-owned
`session.started`, `agent.message`, `agent.activity`, `session.completed`, and
`session.failed` events through their server-side clarification projection after
identity, sequence, payload-integrity, dedupe, and ACK-after-commit handling.
This change SHALL NOT introduce that routing or change the generic rejection
fallback. It SHALL modify only `session.failed`: the pre-retry terminal
logical-run projection becomes an execution-attempt fact handled by the
canonical execution-retry-authority policy. Requirement lifecycle, content,
revision, and readiness remain untouched.

A known failure MAY close/clear the current attempt, preserve a valid
non-cancelled logical run as `phase=active,status=retrying`, and persist
`next_retry_at` when retry budget and owner validity permit. Exhausted failure,
`execution_outcome_unknown`, invalid/revoked owner, or cancellation becomes
terminal `phase=terminal,status=failed`; it releases the sequential slot and
cannot receive `session.resume`. Owner liveness alone does not decide policy.
Transport replay state remains in reconciliation/watermark records, and
replayed facts remain inert.

#### Scenario: Clarification event reaches its owning projection

- **WHEN** a valid `session.started`, `agent.message`, `agent.activity`, or
  `session.completed` event targets an assigned clarification session
- **THEN** server SHALL retain the landed owning projection and ACK-after-commit
  behavior; this retry delta adds no generic-event projection behavior

#### Scenario: Current failure projection is terminal

- **WHEN** `session.failed` is exhausted, unknown-outcome, owner-invalid, or
  terminally cancelled
- **THEN** server SHALL project `phase=terminal,status=failed`, release the
  sequential slot, and reject `session.resume` for that logical run

#### Scenario: Unsupported ownership uses generic rejection

- **WHEN** a well-formed runtime event has no owning clarification projection
- **THEN** server MAY record `event_handler_not_implemented` durably and SHALL
  send a rejected ACK only after that rejection commits

#### Scenario: Failure event changes from terminal run fact to attempt fact

- **WHEN** a valid clarification-owned `session.failed` fact arrives for the
  current execution attempt
- **THEN** the server SHALL record the attempt outcome and clear the current
  attempt before applying retry policy, rather than unconditionally
  terminalizing the logical run

#### Scenario: Known failure remains retrying

- **WHEN** failure is known, the logical run is not cancelled, the pinned owner
  is valid, and retry budget remains
- **THEN** server SHALL persist safe failure state and `next_retry_at`, leave the
  run active/retrying and slot-occupying, and later create one explicit
  `session.resume`

#### Scenario: Unknown outcome is terminal and not resumable

- **WHEN** local reattachment fails and the daemon reports
  `execution_outcome_unknown`
- **THEN** server SHALL terminalize the logical run as failed, create no
  automatic `session.resume`, and reject any later resume for that terminal run;
  a later execution requires a new logical run and explicit `session.start`
