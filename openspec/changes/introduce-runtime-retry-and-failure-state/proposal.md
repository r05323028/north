# Make execution retry authority concrete without adding a second state machine

## Why

The canonical `execution-retry-authority` capability already assigns retry
policy, attempt accounting, restart persistence, `session.resume`, and unknown
outcome handling to the server. The active change's separate `execution-state`
capability duplicates that authority and leaves `session.failed`, scheduling,
cancellation, and public clarification projection ambiguous.

## What changes

- Refine `execution-retry-authority`; remove the overlapping `execution-state`
  delta.
- Define `session.failed` as an execution-attempt fact. The server applies
  policy, keeps retrying runs in the sequential clarification slot, and creates
  durable `session.resume` commands when due.
- Persist attempt identity/accounting, retry due time, safe failure classification,
  and idempotency boundaries transactionally with commands and event handling.
- Define startup/due-retry discovery, concurrent worker claims, pinned-daemon
  behavior, reconnect races, and cancellation races.
- Extend the existing clarification projection with `active/retrying` and
  terminal `failed` mappings plus safe attempt/retry fields. Do not expose a
  second Idle/Running/Retrying/Failed browser truth or raw runtime details.
- Preserve `execution_outcome_unknown`: once it terminalizes a logical run, that
  run can never receive `session.resume`. Later execution creates a new logical
  run with a new `run_id`/protocol `session_id`, current context, new
  `session.start`, and normal slot/state-version rules.

## Capabilities

### Modified Capabilities

- `execution-retry-authority`: durable attempt policy, failure facts,
  scheduling, cancellation, and safe public projection.
- `daemon-protocol`: runtime events hand off to owning server projections
  instead of generic delivery-only rejection; wire schemas remain unchanged.
- Existing clarification-runtime/workspace contracts: retrying and failed
  projections use the same run identity and phase ownership.

No new execution-state capability is added.

## Non-goals

No new execution-state capability, daemon-owned retry policy, automatic daemon
migration, provider registry, generic scheduler framework, HA ownership epochs,
Requirement lifecycle mutation on execution failure, or browser auto-retry.

## Dependencies

Consumes current execution-retry-authority, daemon-runtime, session ownership,
clarification-runtime, and Requirement workspace contracts. It extends their
boundaries; it does not treat completed changes as future prerequisites.

## Documentation impact

Update protocol, daemon, persistence, architecture, lifecycle, testing, and
invariant documentation with the server/daemon boundary, durable schedule,
public projection, and pending implementation status.
