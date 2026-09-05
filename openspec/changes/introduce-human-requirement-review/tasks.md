# Human review implementation tasks

## 1. Contract inventory and shared API types

- [ ] Confirm current `/requirements/{id}/review-packet` response and all four
      mutation routes against `north-server`/OpenAPI-style client types.
- [ ] Add shared typed request/response builders with the exact payload matrix:
      `assessment_id` + `expected_state_version` for Accept/Reject/Request
      Changes; `expected_state_version` only for Reopen.
- [ ] Preserve server 401/403/404/409/error bodies without converting stale
      conflicts into generic success.
- [ ] Add Vitest coverage for request serialization and response parsing.

## 2. Existing workspace presentation

- [ ] Extend only `/requirements/[id]`; do not add `/requirements/[id]/review`
      or another Requirement detail route.
- [ ] Render current packet evidence and consume its canonical concurrency
      identities for mutations in an accessible review panel; do not display
      `assessment_id` or version tokens unless an intentional product/debug view
      already exists. Render Reopen for Rejected Requirements without packet
      dependency.
- [ ] Keep Requesters read-only and show reviewer controls only for
      Requirement Manager/Admin/Owner when lifecycle permits.
- [ ] Keep packet/evidence loading separate from transcript/activity rendering.

## 3. Packet loading and canonical refresh

- [ ] Load Requirement first; request Review Packet only for Ready.
- [ ] Skip packet requests for non-Ready states and render Reopen for Rejected
      from Requirement state/version alone.
- [ ] Surface Ready packet failures as review-load errors that disable Ready
      actions; never infer packet data from messages, activity, or text.
- [ ] On repair, refetch Requirement under a generation, conditionally fetch
      packet, drop old packet when non-Ready, and suppress old responses.
- [ ] Wire SSE hints, reconnect, focus/visibility return, and explicit refresh to
      canonical refetch with request-generation/stale-response suppression.
- [ ] Refetch after every successful review mutation.

## 4. Decisions and feedback

- [ ] Implement Accept, Reject, Request Changes, and Reopen using existing
      routes and lifecycle-specific controls; no optimistic lifecycle updates.
- [ ] Validate Request Changes feedback using existing server rules and send it
      only for that action.
- [ ] Preserve unsent feedback across ordinary refreshes, errors, and stale
      repair; clear only after confirmed successful Request Changes.
- [ ] Disable only the in-flight action and expose accessible pending/error
      state.

## 5. Stale repair and concurrency

- [ ] On HTTP 409, invalidate old packet, refetch Requirement first or through a
      coordinated generation, fetch packet only if Ready, show stale/review-
      required notice, and keep mutations disabled until an explicit accessible
      acknowledgement for that generation (`Review refreshed packet` for Ready,
      `Review refreshed Requirement` for Reopen).
- [ ] Reset that acknowledgement on every later hint/refetch; never auto-retry a
      failed review action.
- [ ] Add Vitest race tests for edit/lifecycle/Ready-generation/assessment
      changes, old-packet rejection, and feedback preservation.

## 6. Audit and safety boundary

- [ ] Verify existing durable audit rows for all four transitions, including
      actor, transition, timestamps, feedback, state-version context, and
      assessment identity where applicable.
- [ ] Do not add a browser history endpoint or call activity rows review audit.
- [ ] Verify no daemon credentials, raw runtime output, hidden reasoning, or
      duplicate Requirement/readiness entity enters the workspace.

## 7. Browser and integration verification

- [ ] Playwright: Ready review in canonical route; requester read-only;
      reviewer decision; Request Changes; Reopen; no duplicate route.
- [ ] Playwright: stale 409 refetch, no optimistic transition, preserved
      textarea, explicit Ready-packet/Reopen-state acknowledgement, and
      missed/duplicate SSE hint repair.
- [ ] PostgreSQL integration: reviewer authorization, packet identity,
      assessment/state-version conflicts, atomic transitions, and durable audit
      writes.
- [ ] Architecture checks: browser HTTP+SSE only and no duplicate ownership.

## 8. Documentation and validation

- [ ] Update lifecycle, roles, architecture, testing, and invariant docs.
- [ ] Run targeted Vitest, Playwright, PostgreSQL integration, and architecture
      tests; do not mark unexecuted layers complete.
- [ ] Run `openspec validate --all --strict` and the relevant `validate.sh`
      profile before implementation is declared complete.
