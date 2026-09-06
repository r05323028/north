# Discord CI status notifications

## Purpose

Provides maintainers timely, non-blocking Discord visibility into completed repository CI runs without making chat delivery part of North's merge gate.

## ADDED Requirements

### Requirement: Completed CI runs notify Discord

The repository SHALL attempt one Discord notification for every completed run of the `CI` workflow. The notification SHALL identify the workflow conclusion, triggering event, branch, run number, and link to the completed GitHub Actions run.

#### Scenario: Successful CI run is reported

- **WHEN** a `CI` workflow run completes with conclusion `success` and `DISCORD_CI_WEBHOOK` is configured
- **THEN** the notification contains successful status and a link to that run

#### Scenario: Failed or cancelled CI run is reported

- **WHEN** a `CI` workflow run completes with any non-success conclusion and `DISCORD_CI_WEBHOOK` is configured
- **THEN** the notification contains the actual conclusion and the completed run link

### Requirement: Webhook configuration stays secret

The notification endpoint SHALL come only from the repository secret `DISCORD_CI_WEBHOOK`. Event-derived values SHALL be encoded as data rather than executable shell or JSON syntax, and the endpoint value SHALL not be written to logs or repository files.

#### Scenario: Missing webhook configuration is safe

- **WHEN** a completed `CI` run has no `DISCORD_CI_WEBHOOK` secret
- **THEN** no Discord request is made and the notification workflow exits without changing the `CI` result

#### Scenario: Untrusted event text is rendered as data

- **WHEN** branch, actor, or other workflow metadata contains quotes, newlines, or shell metacharacters
- **THEN** notification creation remains valid and those values cannot execute commands or alter request structure

### Requirement: Discord delivery is isolated from CI gating

The Discord notification workflow SHALL run after `CI` completion without being a dependency of the `CI` workflow's `gate` job. A Discord request failure MAY fail the notification workflow, but SHALL NOT change, block, or retroactively rewrite the completed `CI` conclusion.

#### Scenario: Discord outage does not change merge status

- **WHEN** the configured Discord endpoint returns an error or is unreachable after `CI` completes
- **THEN** `CI` retains its original conclusion and `gate` remains governed only by its existing required jobs
