# requirement-conversation-workspace Specification Delta

## MODIFIED Requirements

### Requirement: Existing Requirement route becomes a two-part workspace

The existing `/requirements/[id]` route SHALL remain the only Requirement/review
surface. Its Live Requirement area may include the human-review presentation and
actions from the human-review capability; no review sub-route, duplicate
Requirement entity, or review-specific workspace is added. Review actions render
inside the same canonical workspace and use server responses after mutation.

#### Scenario: Deep link loads complete workspace

- **WHEN** an authenticated user opens or refreshes `/requirements/R`
- **THEN** the canonical workspace for R renders, including review presentation
  when R is reviewable, without a second route

#### Scenario: Small screen keeps structured state reachable

- **WHEN** workspace renders below desktop breakpoint
- **THEN** Conversation and Live Requirement/review state remain reachable in one
  route with accessible controls

#### Scenario: Clarification runtime is unavailable

- **WHEN** R has no eligible daemon or pinned daemon is offline
- **THEN** workspace still renders canonical Requirement/conversation and any
  applicable review state without treating runtime absence as page absence

### Requirement: Canonical detail reads repair all workspace state

The workspace SHALL load Requirement first. Only when the canonical Requirement
status is `Ready` SHALL it request `GET /requirements/{id}/review-packet`. A
non-Ready Requirement has no applicable packet request; the server's conflict
for such a request is not a nullable packet contract. A Rejected Requirement
renders Reopen from Requirement state/version alone. If a Ready packet fetch
fails, the workspace surfaces a review-load error and disables Ready review
actions.

Every repair is generation-aware: refresh Requirement first or coordinate both
responses under one generation, then fetch a packet only if refreshed state is
Ready. A non-Ready refresh invalidates/drops any previous packet. No old packet
may be submitted against refreshed Requirement state. After stale review repair,
review mutations remain disabled until the explicit refreshed-state
acknowledgement required by the human-review contract.

#### Scenario: Initial bundle uses canonical endpoints

- **WHEN** workspace first mounts for R
- **THEN** it loads Requirement and other canonical reads, requests Review Packet
  only if R is Ready, and renders no inferred review truth

#### Scenario: Relevant hint causes canonical repair

- **WHEN** a relevant SSE hint arrives
- **THEN** workspace refetches Requirement, conditionally refetches packet for a
  Ready result, and ignores hint payload as state

#### Scenario: Unrelated hint is ignored

- **WHEN** hint names another Requirement
- **THEN** R is unchanged and no packet request is triggered for R solely by it

#### Scenario: Missed and duplicate hints do not duplicate state

- **WHEN** hints are missed or repeated
- **THEN** reconnect/focus repair conditionally reloads canonical packet state once
  per generation without duplicate entities

#### Scenario: Older bundle response cannot overwrite newer state

- **WHEN** an older Requirement or packet response completes after a newer
  generation
- **THEN** older response is ignored and cannot restore an old packet

#### Scenario: Refresh failure keeps usable stale data

- **WHEN** non-initial canonical refetch fails
- **THEN** last-known safe data remains visible with an honest refresh error and
  review actions remain disabled if current packet truth is unavailable

#### Scenario: Ready-only Review Packet loading

- **WHEN** Requirement is Ready
- **THEN** workspace fetches the canonical packet and enables actions only after
  successful packet validation

#### Scenario: Non-Ready Requirements skip Review Packet

- **WHEN** Requirement is Draft, Discussing, Accepted, or another non-Ready state
- **THEN** workspace does not require or request Review Packet; no packet error is
  shown for that state

#### Scenario: Rejected Reopen does not require Review Packet

- **WHEN** Requirement is Rejected
- **THEN** workspace renders Reopen from Requirement state/version alone

#### Scenario: Ready becomes non-Ready during repair

- **WHEN** stale repair changes Ready to Discussing, Rejected, or another state
- **THEN** old packet is dropped, no Ready action remains enabled, and no old
  packet can be submitted

### Requirement: Workspace permissions follow instance roles and server authority

Review presentation/actions in the workspace SHALL follow the existing role matrix.
Requester visibility remains read-only for review; Requirement Manager, Admin,
and Owner may see actions where lifecycle permits. Client gating is UX only and
server authorization is authoritative. Review action success refetches canonical
workspace state; no client transition is authoritative.

#### Scenario: Requester collaborates across ownership

- **WHEN** an authenticated Requester views or edits within existing policy
- **THEN** workspace access follows canonical collaboration rules without creator
  matching

#### Scenario: Reviewer role receives review affordance

- **WHEN** Manager/Admin/Owner views a reviewable Requirement
- **THEN** workspace may expose review actions while server guards remain
  authoritative

#### Scenario: Requester cannot review by forging UI

- **WHEN** Requester calls review mutation directly
- **THEN** server rejects before lifecycle/audit mutation

#### Scenario: Current actor is canonical

- **WHEN** `/auth/me` identifies actor and role
- **THEN** workspace labels and affordances use that response

#### Scenario: Requester edit does not grant reviewer authority

- **WHEN** Requester edits content then attempts review/readiness mutation
- **THEN** content rules and reviewer guards remain separate

#### Scenario: Terminality remains server-enforced

- **WHEN** any role targets invalid lifecycle or run
- **THEN** server rejects and workspace shows authoritative error
