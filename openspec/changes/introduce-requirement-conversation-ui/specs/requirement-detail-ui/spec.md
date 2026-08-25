## Purpose

Renders the clarify loop honestly: dialogue beside living structured state and
coarse activity, with zero exposure of raw model reasoning or tool spam.

## ADDED Requirements

### Requirement: Three-pane detail surface

The detail view SHALL offer Conversation, Overview, and Activity tabs;
conversation posts and replies flow over HTTP+SSE; Overview shows structured
fields verbatim; Activity lists high-level agent events.

#### Scenario: Tabs stay consistent with state

- **WHEN** an assessment promotes the requirement to Ready mid-view
- **THEN** status badges update and the conversation remains scrollable
without losing position

### Requirement: Structured fields render from state, not transcript

Overview content SHALL come exclusively from the structured requirement API.
No part of the specification SHALL be reconstructed by summarizing messages.

#### Scenario: Transcript loss cannot distort spec

- **WHEN** conversation history is unavailable in a fault-injection test
- **THEN** Overview still renders identical structured content

### Requirement: Inline edits honor domain rules

Editing allowed fields inline SHALL call the structured-edit flow: exactly one
revision increment per save, automatic demotion when previously Ready, refusal
in terminal states surfaced as user-visible errors.

#### Scenario: Ready edit demotes visibly

- **WHEN** a requester saves an assumption edit on a Ready requirement
- **THEN** the badge flips to Discussing and a revision notice appears

### Requirement: Telemetry boundaries in the UI

Neither chain-of-thought nor raw tool output SHALL render anywhere in the
detail surface; Activity shows coarse entries only.

#### Scenario: No reasoning leakage

- **WHEN** activity payloads contain internal diagnostics
- **THEN** they render as generic activity summaries or are dropped, never
verbatim
