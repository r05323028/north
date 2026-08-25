## Purpose

Makes forgetting safe: ephemeral runtime telemetry expires on a schedule while
durable product truth remains structurally untouched.

## ADDED Requirements

### Requirement: TTL applies to ephemeral records only

Ephemeral classes (runtime events, tool activity, transient execution logs)
SHALL carry expiry timestamps and be deleted by periodic batched GC after a
configurable retention window. Durable classes (requirements, revisions,
assessments, conversations, messages, repositories, users, roles, review
decisions, daemon registrations) SHALL NOT be eligible for TTL deletion.

#### Scenario: GC sweep deletes only its class

- **WHEN** the retention job runs
- **THEN** expired ephemeral rows disappear and a durable-row count is
byte-identical before and after

### Requirement: Deletion cannot corrupt requirements

After complete deletion of all expired runtime records, every requirement's
structured state, status, revision, and assessment history SHALL remain fully
served and semantically identical.

#### Scenario: Amnesia test

- **WHEN** all ephemeral records within retention horizon are purged
- **THEN** requirement detail, review packets, and boards render identically
to pre-purge snapshots

### Requirement: Configurable retention

Retention window and GC cadence SHALL be configuration with documented
defaults; disabling GC SHALL be possible for operators who choose infinite
horizons.

#### Scenario: Operator stretches retention

- **WHEN** retention is raised and the scheduler restarts
- **THEN** older-than-old-default but newer-than-new records survive
