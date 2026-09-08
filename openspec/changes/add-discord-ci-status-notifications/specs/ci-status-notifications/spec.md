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

### Requirement: Selected pull request events notify Discord

The repository SHALL attempt one Discord notification for each `pull_request` event whose action is `opened`, `synchronize`, `reopened`, `ready_for_review`, or `review_requested`. The notification SHALL identify the action, pull request number and title, base and head branches, actor, and link to the pull request.

#### Scenario: Pull request lifecycle event is reported

- **WHEN** a selected pull request action occurs and `DISCORD_PR_WEBHOOK` is configured
- **THEN** the notification contains the action, pull request context, and link without checking out or executing pull request code

### Requirement: Webhook configuration stays secret

The notification endpoint SHALL come only from `DISCORD_CI_WEBHOOK` for `workflow_run` events or `DISCORD_PR_WEBHOOK` for `pull_request` events. Event-derived values SHALL be encoded as data rather than executable shell or JSON syntax, and endpoint values SHALL not be written to logs or repository files.

#### Scenario: Missing selected webhook configuration is safe

- **WHEN** a supported event has no webhook secret for its event type
- **THEN** no Discord request is made and the notification workflow exits successfully without changing CI or pull request state

#### Scenario: Untrusted event text is rendered as data

- **WHEN** branch, actor, or other workflow metadata contains quotes, newlines, or shell metacharacters
- **THEN** notification creation remains valid and those values cannot execute commands or alter request structure

### Requirement: Discord delivery is isolated from CI gating

The Discord notification workflow SHALL run after `CI` completion without being a dependency of the `CI` workflow's `gate` job. A Discord request failure MAY fail the notification workflow, but SHALL NOT change, block, or retroactively rewrite the completed `CI` conclusion.

#### Scenario: Discord outage does not change merge status

- **WHEN** the configured Discord endpoint returns an error or is unreachable after `CI` completes
- **THEN** `CI` retains its original conclusion and `gate` remains governed only by its existing required jobs
