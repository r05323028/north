# Introduce Requirement domain model and lifecycle

## Why

Requirements are North's core object. Their lifecycle and revision semantics
must be enforced once, in domain code, with persistence that cannot write
illegal states.

## What Changes

- Persist the 0.1.0 Requirement model: title, description, summary,
  acceptance_criteria[], assumptions[], open_questions[], status, revision,
  created_by, timestamps.
- Server-side transition endpoints driven by domain transition table:
  Draft→Discussing; human review transitions gated by role (Accept / Reject /
  Request Changes from Ready; Reopen from Rejected).
- Revision increments on every accepted content edit; terminal states refuse
  edits; editing Ready demotes to Discussing (domain rule surfaced end-to-end).
- Minimal transition audit trail (actor, transition, timestamp).
- List/get APIs with search/filter/sort primitives.

Deliberately deferred here: agent-driven entry into Ready (lands with
introduce-readiness-assessment so the gating contract exists first).

Out of scope: attachments, labels/tags, priorities beyond board columns,
generic PRD schemas.

## Capabilities

### New Capabilities

- `requirements`: model, lifecycle enforcement, revisions, review transitions,
  query surface.

### Modified Capabilities

- `roles`: review-transition endpoints become the first consumers of the
  reviewer gate.

## Impact

- Migration 0003 (requirements + transition audit).
- Affected docs: docs/product/requirement-lifecycle.md stays canonical;
  docs/architecture/persistence.md gains requirements tables.
- Dependencies on earlier changes: introduce-role-and-permission-model.
