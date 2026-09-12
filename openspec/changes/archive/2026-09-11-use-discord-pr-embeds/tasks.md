# Discord PR embed notifications

## 1. OpenSpec and workflow

- [x] 1.1 Add the PR embed capability spec, design, and workflow tasks for lifecycle and completed-check notifications.
- [x] 1.2 Replace raw `/github` forwarding with standard Discord embed payloads, preserving safe `jq` encoding, bounded retries, optional secrets, no checkout, and CI-gate isolation.
- [x] 1.3 Add read-only PR metadata lookup for PR-associated completed CI runs and render status, author, workflow run, source branch, and links.

## 2. Documentation and verification

- [x] 2.1 Update `docs/development/ci.md` with base webhook setup, migration behavior, embed fields, trigger coverage, and failure isolation.
- [x] 2.2 Validate YAML/script structure, secret isolation, payload safety, and OpenSpec strict validation; run targeted live webhook and repository checks.
