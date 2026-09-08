# Discord CI status notifications

## 1. Discord workflow

- [x] 1.1 Add `.github/workflows/discord-ci-status.yml` with a `workflow_run` completion trigger for `CI`, optional `DISCORD_CI_WEBHOOK` secret handling, safe status payload construction, bounded delivery retries, and no dependency on `CI.gate`.
- [x] 1.2 Validate workflow structure, secret isolation, event-derived JSON encoding, and failure isolation for CI and pull request notifications with targeted static checks plus `git diff --check`.
- [x] 1.3 Add `pull_request` notifications for `opened`, `synchronize`, `reopened`, `ready_for_review`, and `review_requested` without checkout or pull request code execution.

## 2. Repository documentation

- [x] 2.1 Update `docs/development/ci.md` with secret setup, notification contents, event coverage, missing-secret behavior, and rollback/failure isolation.
- [x] 2.2 Run `openspec validate --all --strict` and the relevant repository validation profile; review the final diff for scope and stale claims.
