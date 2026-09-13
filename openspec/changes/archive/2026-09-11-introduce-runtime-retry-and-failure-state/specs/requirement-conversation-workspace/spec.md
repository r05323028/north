# requirement-conversation-workspace Specification Delta

## MODIFIED Requirements

### Requirement: Clarification phase, status, completion, and cancellation remain separate

The workspace SHALL consume the existing phase/status projection plus safe retry
fields without owning retry policy. It SHALL render
`phase=active,status=retrying` as an active slot-occupying server retry and
`phase=terminal,status=failed` as terminal execution failure/cancellation. It
SHALL never auto-submit `session.resume`, create a new run from a browser retry,
or infer failure/retry from transcript/activity. Unknown-outcome terminal runs
cannot be resumed; a later execution starts through a new logical run.

#### Scenario: Completion without assessment is not Ready

- **WHEN** a run completes before accepted readiness assessment
- **THEN** workspace shows completion without synthetic Ready state

#### Scenario: Readiness changes independently

- **WHEN** readiness is stale/rejected or targets another revision
- **THEN** workspace uses canonical readiness and does not infer from runtime text

#### Scenario: Assigned cancellation waits for terminal fact

- **WHEN** assigned cancellation is acknowledged without terminal runtime fact
- **THEN** workspace keeps the run active, blocks later dispatch, and does not
  label ACK as completion

#### Scenario: Successful cancellation is distinct from failure

- **WHEN** cancellation projects successful `session.completed`
- **THEN** workspace shows terminal/completed with cancellation intent

#### Scenario: Cancellation preserves committed partial state

- **WHEN** cancellation follows persisted messages/activity/readiness data
- **THEN** committed facts remain visible and are not rolled back

#### Scenario: Runtime failure leaves Requirement lifecycle alone

- **WHEN** execution reaches terminal/failed or active/retrying
- **THEN** workspace leaves Requirement lifecycle, revision, readiness, and state
  version unchanged

#### Scenario: Retry state remains active

- **WHEN** server policy schedules a retry after a known attempt failure
- **THEN** the workspace shows active/retrying, keeps the sequential slot occupied,
  and exposes no browser retry action or raw runtime reason

#### Scenario: Terminal retry failure releases the slot

- **WHEN** server policy exhausts attempts or rejects an unknown outcome
- **THEN** the workspace shows terminal/failed, permits a later new run, and leaves
  Requirement lifecycle, revision, and state version unchanged

#### Scenario: Unassigned cancellation releases slot

- **WHEN** awaiting run is cancelled before start command
- **THEN** workspace shows terminal cancellation and permits later new run

#### Scenario: Retry projection retains the sequential slot

- **WHEN** server schedules a known retry
- **THEN** workspace shows active/retrying and disables new-run start

#### Scenario: Unknown outcome cannot resume terminal run

- **WHEN** the latest run is terminal/failed for unknown outcome
- **THEN** workspace offers no resume action; later execution uses a new start/run
