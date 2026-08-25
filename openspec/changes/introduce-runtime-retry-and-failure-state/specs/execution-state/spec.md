## Purpose

Separates infrastructure health from business truth: executions retry
deterministically, fail only on exhausted budgets, and never drag requirement
lifecycle states along.

## ADDED Requirements

### Requirement: Independent execution state machine

Each active session SHALL expose Idle/Running/Retrying/Failed independent of
requirement lifecycle. Changing execution state MUST NOT change requirement
status, and vice versa.

#### Scenario: Failed run leaves requirement untouched

- **WHEN** an execution exhausts retries and becomes Failed
- **THEN** the related requirement's status/revision are byte-identical to
before the run started

### Requirement: Bounded retry with backoff before Failed

Transient failures (disconnects, runtime crashes) SHALL retry up to a
configured bound with exponential backoff; resumption SHOULD reuse
session.resume where safe. Only exhaustion SHALL produce Failed, recorded
with reason and attempt count.

#### Scenario: Single blip does not fail work

- **WHEN** connectivity drops once and returns within the backoff window
- **THEN** the session resumes Running and no Failed state ever appears

#### Scenario: Exhaustion fails honestly

- **WHEN** every retry attempt fails
- **THEN** execution becomes Failed with the last error and attempt count
visible

### Requirement: Configurable, documented policy

Retry bounds and delays SHALL be configuration (not hard-coded magic), with
defaults documented so operators can tune them.

#### Scenario: Operator tunes patience

- **WHEN** max_attempts is lowered in config and a flaky run occurs
- **THEN** behavior matches the new bound without code changes
