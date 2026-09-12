# ci-pr-embeds Specification

## Purpose

Provides review-focused Discord messages that make pull-request activity and completed checks easy to scan without opening GitHub.

## Requirements

### Requirement: Pull-request activity uses rich embeds

For each selected pull-request action (`opened`, `synchronize`, `reopened`, `ready_for_review`, or `review_requested`), the notification SHALL be delivered as a standard Discord webhook embed to the configured PR webhook. The embed SHALL identify the repository, action, pull-request number and title, pull-request author, source branch, and pull-request URL.

#### Scenario: Pull-request action appears in review channel

- **WHEN** a selected pull-request action occurs and `DISCORD_PR_WEBHOOK` is configured
- **THEN** `#pr-review` receives one rich embed containing the action and linked pull-request context

#### Scenario: Pull-request title contains special characters

- **WHEN** pull-request metadata contains quotes, newlines, or shell metacharacters
- **THEN** the embed remains valid and metadata is rendered as data without command execution or payload corruption

### Requirement: Pull-request checks use status embeds

When a completed `CI` run is associated with a pull request, the notification SHALL send one review-channel embed whose title identifies the repository, check conclusion, pull-request number, and pull-request title. The embed SHALL include the completed run description and linked fields for PR Author and Workflow Run plus the source branch.

#### Scenario: Successful pull-request checks are reported

- **WHEN** a `CI` run associated with a pull request completes successfully and `DISCORD_PR_WEBHOOK` is configured
- **THEN** the review-channel embed reports `Checks Successful on PR`, links the pull request and workflow run, and uses a success color

#### Scenario: Unsuccessful pull-request checks are reported

- **WHEN** a pull-request-associated `CI` run completes with failure, cancellation, timeout, or action-required conclusion
- **THEN** the review-channel embed reports the actual check status and uses a non-success status color

### Requirement: PR embed delivery remains optional and isolated

The PR notification endpoint SHALL come only from `DISCORD_PR_WEBHOOK`, SHALL support a normal Discord webhook URL, and SHALL not require the GitHub-compatible `/github` suffix. A legacy trailing `/github` suffix MAY be tolerated during migration. Missing configuration SHALL skip delivery; delivery failure SHALL not change the `CI` workflow conclusion or required `gate` result. The workflow SHALL use metadata only and SHALL not check out or execute pull-request code.

#### Scenario: Missing PR webhook is safe

- **WHEN** a selected pull-request event or associated completed CI run has no `DISCORD_PR_WEBHOOK`
- **THEN** no PR webhook request is made and the notification workflow succeeds without changing repository or CI state

#### Scenario: Discord outage does not affect CI

- **WHEN** the configured PR webhook is unavailable or returns an error
- **THEN** the notification attempt fails visibly after bounded retries while the completed `CI` result and `gate` remain unchanged
