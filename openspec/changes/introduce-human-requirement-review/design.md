# Design: human Requirement review in canonical workspace

## Boundary

The existing browser route `/requirements/[id]` remains the only Requirement
detail surface. Its existing requester workspace owns Requirement content,
conversation, readiness, activity, clarification run, and current-user
identity. Human review is one additional panel/state inside that workspace.

No `/requirements/[id]/review`, alternate detail route, review-specific
workspace, browser WebSocket, or client-owned Requirement/readiness store is
introduced.

The server contracts already on `main` remain authoritative:

- `GET /requirements/{id}/review-packet` returns the current review evidence and
  concurrency identities.
- Accept, Reject, Request Changes, and Reopen routes enforce reviewer roles and
  expected state versions.
- Accept, Reject, and Request Changes bind `assessment_id`; Reopen does not.
- The server validates Ready state, revision, Ready generation, assessment
  identity, and `expected_state_version` atomically, then writes durable audit
  data.

The browser consumes these contracts; it does not redesign them.

## Data ownership and loading

The workspace keeps two canonical reads:

1. Requirement data supplies current content/lifecycle/state version.
2. Review Packet supplies review evidence, assessment identity, Ready-generation
   identity, and review-specific concurrency values.

Review controls are derived from those responses and `/auth/me` only for UX.
The browser never infers Ready from transcript text, activity, or an assessment
message and never reconstructs packet evidence from another read.

Loading is conditional and generation-aware. The workspace first loads the
canonical Requirement. Only when that response says `Ready` does it request
`GET /requirements/{id}/review-packet`. Draft, Discussing, Accepted, and other
non-Ready states do not require a packet request; Rejected renders Reopen from
Requirement state/version alone. A Ready packet fetch failure is surfaced as a
review-load error and disables Ready actions.

On repair, the browser refetches Requirement first or coordinates responses
under one request generation, then fetches a packet only if the refreshed
Requirement is Ready. A refreshed non-Ready state drops the old packet. An old
packet is never submitted against refreshed Requirement state.

SSE `requirement.changed`/readiness hints, reconnect, focus/visibility return,
and an explicit refresh button schedule/refetch canonical HTTP state. SSE does
not carry packet truth. Duplicate hints are harmless, and an older response
cannot overwrite a newer request generation.

## Review action contract

| Action | Eligible lifecycle | Request body |
| --- | --- | --- |
| Accept | Ready | `assessment_id`, `expected_state_version` |
| Reject | Ready | `assessment_id`, `expected_state_version` |
| Request Changes | Ready | `assessment_id`, `expected_state_version`, feedback |
| Reopen | Rejected | `expected_state_version` |

The shared API layer must have separate typed builders or an equivalent
conditional body so Reopen cannot accidentally receive or require
`assessment_id`, and the three Ready decisions cannot omit it. Feedback is
trimmed/validated according to the existing server contract; it is not sent for
other actions.

The workspace does not optimistically change lifecycle, readiness, state
version, or packet identity. On success it refetches canonical Requirement and
all applicable reads; it fetches Review Packet only when the refreshed
Requirement is Ready and renders the server result.

## Permission matrix

| Viewer | Read workspace/packet | Review controls |
| --- | --- | --- |
| Requester | Yes, subject to existing workspace access | No |
| Requirement Manager | Yes | Yes where lifecycle permits |
| Admin | Yes | Yes where lifecycle permits |
| Owner | Yes | Yes where lifecycle permits |

This is presentation gating only. Every mutation still handles server 401,
403, 404, 409, and lifecycle errors without assuming the client check was
security.

## Stale packet repair

The browser follows this exact flow:

1. Load current Requirement and packet.
2. Reviewer edits or inspects locally.
3. Another edit, lifecycle transition, Ready-generation change, or assessment
   identity change makes the packet stale.
4. Mutation returns HTTP 409.
5. Browser does not apply an optimistic transition and does not retry the
   mutation.
6. Browser refetches Requirement and packet, marks the old packet unusable, and
   shows a stale/review-required notice.
7. Any unsent Request Changes textarea remains intact.
8. Browser keeps review mutations disabled behind an explicit accessible
   acknowledgement for the refreshed canonical state: `Review refreshed packet`
   when Ready, or `Review refreshed Requirement` when Rejected/Reopen is shown.
   Refetch/render completion alone is not inspection; acknowledgement clears the
   gate only for the current Requirement generation and, when Ready, packet
   generation. Any later hint or refetch sets the gate again.

A successful refetch that is no longer Ready removes Ready actions. A stale
response cannot be used with a refreshed Requirement, even if the old action
would otherwise look valid.

## Audit and history boundary

Existing server transition transactions remain the audit authority. The
browser does not claim that conversation activity is audit history. Current
`main` has no browser-readable review-audit projection beyond durable server
rows, so this change adds no history panel or new audit read route. Tasks verify
that the existing rows retain actor, transition, relevant feedback, assessment
identity where applicable, state-version context, and timestamp.

If a later product requirement needs reviewer-visible history, it needs a
separate read-model decision and contract; this change does not smuggle it in.

## Error and accessibility behavior

Controls are disabled while their own mutation is in flight, not while an
unrelated read is pending. Errors are announced in the existing workspace
status region, remain associated with the action, and do not erase feedback.
Buttons have explicit labels and lifecycle/permission explanations. No server
secret, raw runtime detail, or hidden reasoning is rendered.
