## Purpose

Clarifies that activity telemetry is best-effort and may expire without
changing canonical workspace truth.

## MODIFIED Requirements

### Requirement: Activity, errors, privacy, and accessibility are actionable

The workspace SHALL display only server-published coarse activity summaries and
safe readiness/session state. It SHALL never render chain-of-thought, raw
prompts, hidden model context, raw tool-call traces, credentials, checkout
paths, provider SDK records, daemon logs, or unfiltered failure diagnostics.
Message bodies SHALL be rendered as text, not trusted HTML. Event IDs, daemon
IDs, runtime IDs, and command payloads SHALL not be presented as user-facing
execution controls.

User-visible errors SHALL distinguish at least: message submission failure,
clarification start/dispatch conflict, runtime unavailable before assignment,
pinned runtime unavailable, run failure, cancellation pending/completion/failure,
canonical Requirement refresh failure, and temporary SSE reconnect. A stale
canonical Requirement SHALL remain visible during non-initial failure where
possible. Recovery controls SHALL be explicit and safe: retry the same
unassigned start, retry a failed HTTP read, reconcile an edit, or cancel the
known run; there SHALL be no blind state-version retry, frontend polling loop,
or automatic new-run fallback.

Composer controls, status announcements, tabs/sections, error messages, and
activity updates SHALL have accessible names, keyboard operation, visible focus,
and appropriate live/status semantics. Responsive layout SHALL not depend on
hover or color alone to communicate availability, cancellation, failure, or
readiness.

Coarse activity display is best-effort observability. Activity rows may expire
under bounded retention; conversation history, readiness evidence, the
clarification-run projection, and review state SHALL remain unaffected, and the
workspace SHALL render a valid empty or partial activity history without
treating expiry as an error.

#### Scenario: Raw runtime detail is not exposed

- **WHEN** an upstream runtime produces tool output, hidden reasoning, a checkout path, or a provider-specific error
- **THEN** the workspace omits it or displays only an intentional coarse server summary

#### Scenario: Errors identify recoverable action

- **WHEN** message persistence fails, a start returns 503, an edit returns 409, or SSE reconnects
- **THEN** the workspace shows different actionable states for those conditions and never tells the user that an unsent or stale operation succeeded

#### Scenario: Stale data is honest during refresh outage

- **WHEN** a refetch fails after canonical data was already rendered
- **THEN** that data stays visible with a separate refresh warning, and no local update claims it is newer than the last successful server read

#### Scenario: Keyboard user can operate composer

- **WHEN** a keyboard-only user reaches the Conversation pane
- **THEN** the message field, submit/retry/cancel controls, status announcement, and any activity/Requirement navigation have accessible names and can be operated without pointer hover

#### Scenario: Expired activity does not change canonical workspace state

- **WHEN** older activity rows expire and are swept
- **THEN** conversation, readiness/review, and session reads are unchanged and the workspace still renders
