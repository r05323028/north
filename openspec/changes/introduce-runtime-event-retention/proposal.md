# Introduce runtime-event retention

## Why

Runtime telemetry grows forever unless something boring deletes it. Retention
must be able to erase every ephemeral byte without touching product truth.

## What Changes

- TTL on ephemeral runtime records (runtime events, tool activity, transient
  logs) with configurable retention window.
- Periodic GC deleting expired ephemeral rows only — durable classes are
  untouchable by construction.
- Documentation of durable vs ephemeral classes as enforced, not aspirational.

## Capabilities

### New Capabilities

- `runtime-retention`: TTL application, batched GC, configuration, and the
  durability firewall between the two data classes.

### Modified Capabilities

(none)

## Impact

- Affected docs: docs/architecture/persistence.md (retention mechanics),
  docs/development/invariants.md rows 3/12.
- Dependencies on earlier changes: introduce-agent-requirement-clarification
  (runtime events exist to retain).
