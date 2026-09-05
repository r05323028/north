# Introduce human Requirement review

## Why

Ready hands off to humans; the review moment must be concise, permissioned,
and structurally incapable of approving a stale assessment.

## What Changes

- Review surface for Ready requirements showing the review packet: Goal,
  Scope, Acceptance Criteria, Assumptions, Blocking Questions, Repositories
  Inspected.
- Actions Accept / Request Changes (with feedback) / Reject for reviewers
  only; Reopen on Rejected requirements.
- Stale protection: review actions require `expected_state_version` and validate the
  requirement/assessment revision atomically; a moved revision returns HTTP 409,
  forces re-read, and refuses blind approval.
- Decisions recorded with reviewer identity and timestamp (audit trail).

## Capabilities

### New Capabilities

- `human-review`: packet-driven review flow, decision endpoints, staleness
  guard, reopen path.

### Modified Capabilities

- `requirements`: review transitions gain their canonical UI entry point.

## Impact

- Affected docs: docs/product/requirement-lifecycle.md (review ownership),
  docs/development/invariants.md (stale-approval row).
- Dependencies on earlier changes: introduce-readiness-assessment,
  introduce-requirement-conversation-workspace.
