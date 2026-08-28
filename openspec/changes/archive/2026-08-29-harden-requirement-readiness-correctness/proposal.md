## Why

Requirement content revision is not lifecycle state revision. Current lifecycle endpoints therefore accept stale reviewer commands when a requirement leaves and re-enters Ready at the same content revision, allowing a decision against evidence the reviewer did not inspect. The API also lacks an explicit workspace-wide access contract and cannot clear optional summaries.

## What Changes

- Add positive `state_version` to Requirement persistence and DTOs, separate from content `revision`.
- Require atomic `expected_state_version` for every existing-Requirement mutation; increment it once for every real persisted mutation and never for no-ops, rejected assessments, or duplicate events.
- Bind review Accept/Reject/Request Changes to the current `assessment_id`, content revision, Ready state generation, and expected state version.
- Preserve transactional readiness ingestion, immutable revision-bound evidence, event deduplication, session binding, and ACK-after-commit behavior.
- Make workspace-wide Requirement visibility and collaborative requester access explicit; keep human review reviewer-gated.
- Permit empty `summary` and intentionally empty list fields while retaining non-empty validation for title, description, and list entries.
- Add a migration, regression coverage for stale review and all version semantics, and update canonical specs/product/architecture documentation.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `requirements`: distinguish canonical content revision from mutable state version and define atomic lifecycle concurrency and workspace-wide access.
- `readiness`: bind review packets and decisions to assessment identity plus current Requirement state/version while preserving transaction invariants.
- `conversations`: use state-version concurrency for structured edits and retain conversation as non-canonical context with explicit collaborative access.

## Impact

Affects `north-domain`, `north-persistence`, `north-server`, readiness/review DTOs and routes, migration SQL, integration/unit tests, validation scripts, and product/architecture documentation. No new dependency or ACL subsystem. Existing pre-0.1.0 `expected_revision` mutation semantics are replaced with `expected_state_version`.
