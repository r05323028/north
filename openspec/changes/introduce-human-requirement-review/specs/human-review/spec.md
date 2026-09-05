# human-review Specification Delta

## Purpose

Presents existing human review contracts inside the canonical Requirement
Conversation Workspace without creating a second review truth or route.

## ADDED Requirements

### Requirement: The canonical workspace is the only browser review surface

The browser SHALL render human review in `/requirements/[id]`, alongside the
existing Requirement, conversation, readiness, activity, and clarification
workspace. It SHALL NOT add a review sub-route, alternate Requirement detail
route, review-specific workspace, or browser-to-daemon connection.

#### Scenario: Ready Requirement opens review in place

- **WHEN** a user opens `/requirements/{id}` for a Ready Requirement
- **THEN** the existing workspace renders review content and eligible actions in
  that route, with no navigation to a second review surface

#### Scenario: Rejected Requirement reopens in place

- **WHEN** a reviewer opens a Rejected Requirement
- **THEN** the same workspace offers Reopen when authorized and creates no
  review-specific route or entity

### Requirement: Review Packet response is review truth

The workspace SHALL obtain review evidence, `assessment_id`, Ready-generation
identity, and review concurrency values directly from
`GET /requirements/{id}/review-packet`. It SHALL use the canonical Requirement
read for current content/lifecycle and SHALL NOT reconstruct a packet from
conversation messages, activity, transcript text, or client-owned readiness
entities. Missing/non-current packets SHALL disable review actions rather than
being inferred.

#### Scenario: Transcript does not synthesize review evidence

- **WHEN** conversation contains an assessment-looking agent message without a
  current review packet
- **THEN** the workspace shows no review action based on that message

#### Scenario: Packet refresh replaces old truth

- **WHEN** the workspace refetches a review packet
- **THEN** the old packet cannot be submitted with the refreshed Requirement
  state, and controls use only the refreshed response

### Requirement: Review mutation identities match existing server contracts

For Accept, Reject, and Request Changes, the workspace SHALL send both
`assessment_id` from the current packet and `expected_state_version` from that
packet/Requirement contract. Request Changes SHALL also send its validated
feedback. For Reopen, the workspace SHALL send `expected_state_version` and
SHALL NOT require or send `assessment_id`.

The browser SHALL not optimistically change lifecycle or readiness. On success
it SHALL refetch canonical Requirement and packet state.

#### Scenario: Ready decision carries assessment identity

- **WHEN** an authorized reviewer accepts, rejects, or requests changes for a
  current Ready packet
- **THEN** the request contains that packet's `assessment_id` and exact
  `expected_state_version`

#### Scenario: Reopen has no assessment identity

- **WHEN** an authorized reviewer reopens a Rejected Requirement
- **THEN** the request contains only the current expected state version among
  review concurrency identities

#### Scenario: Success uses server projection

- **WHEN** a review mutation succeeds
- **THEN** the workspace displays the refetched server Requirement/packet state
  and never a client-invented transition

### Requirement: Review controls follow role and lifecycle permissions

Requesters SHALL be able to read the existing workspace subject to existing
access rules but SHALL see no actionable review controls. Requirement Manager,
Admin, and Owner users MAY see and execute controls only where current
lifecycle permits. These checks are UX gating; server authorization remains
mandatory and authoritative.

#### Scenario: Requester is read-only for review

- **WHEN** a Requester views a Ready or Rejected Requirement
- **THEN** review controls are absent or non-actionable, and a forged mutation
  still receives server authorization failure

#### Scenario: Client/server permission disagreement is safe

- **WHEN** a client incorrectly shows a review control to an unauthorized user
- **THEN** the server rejects the mutation and the workspace changes no
  Requirement or packet state

### Requirement: Stale review repair is explicit and preserves feedback

When a review mutation returns HTTP 409 because Requirement content, lifecycle,
Ready generation, assessment identity, or state version changed, the workspace
SHALL leave the old packet unusable, apply no optimistic transition, refetch
canonical Requirement and Review Packet, and require explicit reviewer
inspection before enabling a new review mutation. It SHALL NOT automatically
retry the failed action. Unsent Request Changes feedback SHALL survive the
refetch and stale notice.

#### Scenario: Concurrent edit causes stale decision

- **GIVEN** a reviewer loaded a current packet and another operation changes
  Requirement content, lifecycle, Ready generation, or assessment
- **WHEN** the reviewer submits the old packet and receives HTTP 409
- **THEN** no lifecycle transition is applied in the browser, the Requirement
  and packet are refetched, and the old packet cannot be retried

#### Scenario: Request Changes draft survives stale repair

- **WHEN** Request Changes feedback is unsent and its stale submission returns
  HTTP 409
- **THEN** the textarea retains its text while canonical data refreshes, and no
  automatic resubmission occurs

#### Scenario: Refreshed packet is no longer reviewable

- **WHEN** stale repair finds the Requirement no longer Ready or no longer
  reviewable for that assessment
- **THEN** Ready actions remain disabled and the reviewer sees the refreshed
  canonical reason

### Requirement: Durable review audit remains server-owned

Successful Accept, Reject, Request Changes, and Reopen operations SHALL retain
the existing durable server audit write. The browser SHALL not label coarse
conversation/activity records as review audit and SHALL not promise a
review-history read projection that does not exist in the current server
contract.

#### Scenario: Decision audit is durable

- **WHEN** a review transition commits
- **THEN** the existing server audit transaction records the actor, transition,
  state-version context, timestamp, feedback where supplied, and
  `assessment_id` for Accept, Reject, and Request Changes

#### Scenario: No implicit history subsystem

- **WHEN** the workspace renders current review state
- **THEN** it renders packet/Requirement truth only and does not invent a new
  audit endpoint or history model

### Requirement: Canonical refresh repairs review hints

The workspace SHALL treat SSE review/readiness hints, reconnect, focus/visibility
return, and explicit refresh as reasons to refetch canonical Requirement and
Review Packet state. Hints SHALL not carry or become review truth; duplicate,
late, and missed hints SHALL be safe.

#### Scenario: Missed hint is repaired

- **WHEN** the browser reconnects or returns to focus after a possible review
  change
- **THEN** it refetches canonical Requirement and packet state without using a
  daemon connection or transcript inference
