# Spec Delta

## MODIFIED Requirements

### Requirement: Ephemeral expiry is explicit and deterministic

Allowlisted ephemeral rows SHALL carry `expires_at`, written at insert time from the configured retention window; eligibility SHALL be `expires_at <= CURRENT_TIMESTAMP`. The 0.1.0 baseline SHALL create `expires_at` as NOT NULL so no future insert can bypass expiry treatment. North SHALL NOT claim an upgrade/backfill path for activity rows created by pre-0.1.0 schemas; those databases require manual recreation before first-release use.

#### Scenario: Expired activity is eligible, future activity is not

- **WHEN** the sweep runs with one activity row already expired and one row whose expiry is in the future
- **THEN** the expired row is deleted and the future row remains

#### Scenario: Migration backfills existing activity deterministically

- **WHEN** migration 0017 encounters existing activity in a pre-0.1.0 database
- **THEN** those databases are unsupported by the 0.1.0 baseline; North does not backfill, reset, or rewrite them, and operators must manually recreate them

#### Scenario: Fresh baseline requires expiry

- **WHEN** the 0.1.0 baseline is applied to an empty PostgreSQL database
- **THEN** the activity table requires `expires_at`, and a later insert without expiry is rejected
